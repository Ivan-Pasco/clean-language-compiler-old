use std::collections::HashMap;
use crate::ast::{Expression, Type, Function, Statement};
use crate::error::{CompilerError, CompilerResult};

/// Type checker for semantic analysis
pub struct TypeChecker {
    symbol_table: HashMap<String, Type>,
    function_table: HashMap<String, FunctionType>,
    current_function_return_type: Option<Type>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionType {
    pub params: Vec<Type>,
    pub return_type: Type,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            symbol_table: HashMap::new(),
            function_table: HashMap::new(),
            current_function_return_type: None,
        }
    }

    pub fn check_program(&mut self, statements: &[Statement]) -> CompilerResult<()> {
        // First pass: collect function declarations
        for stmt in statements {
            if let Statement::FunctionDeclaration { function, .. } = stmt {
                self.register_function(function)?;
            }
        }

        // Second pass: check all statements
        for stmt in statements {
            self.check_statement(stmt)?;
        }

        Ok(())
    }

    fn register_function(&mut self, func: &Function) -> CompilerResult<()> {
        let func_type = FunctionType {
            params: func.parameters.iter().map(|p| p.type_).collect(),
            return_type: func.return_type,
        };

        if self.function_table.insert(func.name.clone(), func_type).is_some() {
            return Err(CompilerError::undefined_function(
                &func.name,
                func.location.as_ref().map(|l| l.line).unwrap_or(0),
                func.location.as_ref().map(|l| l.column).unwrap_or(0),
            ));
        }

        Ok(())
    }

    fn check_statement(&mut self, stmt: &Statement) -> CompilerResult<()> {
        match stmt {
            Statement::Let { name, type_, init, location } => {
                if let Some(init_expr) = init {
                    let expr_type = self.infer_type(init_expr)?;
                    if expr_type != *type_ {
                        return Err(CompilerError::type_error(
                            location.line,
                            location.column,
                            format!("Cannot initialize variable of type {:?} with expression of type {:?}", type_, expr_type),
                        ));
                    }
                }
                self.symbol_table.insert(name.clone(), *type_);
                Ok(())
            }
            Statement::Assign { target, value, location } => {
                let target_type = self.lookup_variable(target, location.line, location.column)?;
                let value_type = self.infer_type(value)?;
                if target_type != value_type {
                    return Err(CompilerError::type_error(
                        location.line,
                        location.column,
                        format!("Cannot assign value of type {:?} to variable of type {:?}", value_type, target_type),
                    ));
                }
                Ok(())
            }
            Statement::FunctionDef(func) => {
                let mut checker = TypeChecker::new();
                // Set current function return type context
                checker.current_function_return_type = Some(func.return_type);
                // Add parameters to local scope
                for param in &func.params {
                    checker.symbol_table.insert(param.name.clone(), param.type_);
                }
                // Check function body
                for stmt in &func.body {
                    checker.check_statement(stmt)?;
                }
                Ok(())
            }
            Statement::Return { expr, location } => {
                if let Some(return_type) = &self.current_function_return_type {
                    if let Some(expr) = expr {
                        let expr_type = self.infer_type(expr)?;
                        if !self.types_compatible(return_type, &expr_type) {
                            return Err(CompilerError::type_error(
                                location.as_ref().map(|l| l.line).unwrap_or(0),
                                location.as_ref().map(|l| l.column).unwrap_or(0),
                                format!("Return type mismatch: expected {:?}, found {:?}", return_type, expr_type),
                            ));
                        }
                    } else if *return_type != Type::Void {
                        return Err(CompilerError::type_error(
                            location.as_ref().map(|l| l.line).unwrap_or(0),
                            location.as_ref().map(|l| l.column).unwrap_or(0),
                            format!("Function expects return value of type {:?} but got void return", return_type),
                        ));
                    }
                } else {
                    return Err(CompilerError::type_error(
                        location.as_ref().map(|l| l.line).unwrap_or(0),
                        location.as_ref().map(|l| l.column).unwrap_or(0),
                        "Return statement outside of function".to_string(),
                    ));
                }
                Ok(())
            }
            Statement::If { condition, then_branch, else_branch, location } => {
                let cond_type = self.infer_type(condition)?;
                if cond_type != Type::Bool {
                    return Err(CompilerError::type_error(
                        location.line,
                        location.column,
                        "If condition must be a boolean expression".to_string(),
                    ));
                }
                for stmt in then_branch {
                    self.check_statement(stmt)?;
                }
                if let Some(else_stmts) = else_branch {
                    for stmt in else_stmts {
                        self.check_statement(stmt)?;
                    }
                }
                Ok(())
            }
            Statement::Expression(expr) => {
                self.infer_type(expr)?;
                Ok(())
            }
        }
    }

    fn infer_type(&self, expr: &Expr) -> CompilerResult<Type> {
        match expr {
            Expr::Number(_) => Ok(Type::Number),
            Expr::String(_) => Ok(Type::String),
            Expr::Bool(_) => Ok(Type::Bool),
            Expr::Variable { name, location } => {
                self.lookup_variable(name, location.line, location.column)
            }
            Expr::Binary { op, left, right, location } => {
                let left_type = self.infer_type(left)?;
                let right_type = self.infer_type(right)?;
                match op.as_str() {
                    "+" | "-" | "*" | "/" => {
                        if left_type == Type::Number && right_type == Type::Number {
                            Ok(Type::Number)
                        } else {
                            Err(CompilerError::type_error(
                                location.line,
                                location.column,
                                format!("Cannot perform arithmetic operation on types {:?} and {:?}", left_type, right_type),
                            ))
                        }
                    }
                    "==" | "!=" | "<" | "<=" | ">" | ">=" => {
                        if left_type == right_type {
                            Ok(Type::Bool)
                        } else {
                            Err(CompilerError::type_error(
                                location.line,
                                location.column,
                                format!("Cannot compare values of different types: {:?} and {:?}", left_type, right_type),
                            ))
                        }
                    }
                    "&&" | "||" => {
                        if left_type == Type::Bool && right_type == Type::Bool {
                            Ok(Type::Bool)
                        } else {
                            Err(CompilerError::type_error(
                                location.line,
                                location.column,
                                "Logical operators require boolean operands".to_string(),
                            ))
                        }
                    }
                    _ => Err(CompilerError::type_error(
                        location.line,
                        location.column,
                        format!("Unknown operator: {op}"),
                    )),
                }
            }
            Expr::Call { function, args, location } => {
                if let Some(func_type) = self.function_table.get(function) {
                    if args.len() != func_type.params.len() {
                        return Err(CompilerError::type_error(
                            location.line,
                            location.column,
                            format!("Function {} expects {} arguments but got {}", function, func_type.params.len(), args.len()),
                        ));
                    }
                    for (arg, expected_type) in args.iter().zip(func_type.params.iter()) {
                        let arg_type = self.infer_type(arg)?;
                        if arg_type != *expected_type {
                            return Err(CompilerError::type_error(
                                location.line,
                                location.column,
                                format!("Expected argument of type {:?} but got {:?}", expected_type, arg_type),
                            ));
                        }
                    }
                    Ok(func_type.return_type.clone())
                } else {
                    Err(CompilerError::undefined_function(
                        function,
                        location.line,
                        location.column,
                    ))
                }
            }
            Expr::Matrix { rows, location } => {
                if rows.is_empty() {
                    return Ok(Type::Matrix);
                }
                let row_len = rows[0].len();
                for row in rows {
                    if row.len() != row_len {
                        return Err(CompilerError::type_error(
                            location.line,
                            location.column,
                            "All matrix rows must have the same length".to_string(),
                        ));
                    }
                    for elem in row {
                        let elem_type = self.infer_type(elem)?;
                        if elem_type != Type::Number {
                            return Err(CompilerError::type_error(
                                location.line,
                                location.column,
                                "Matrix elements must be numbers".to_string(),
                            ));
                        }
                    }
                }
                Ok(Type::Matrix)
            }
        }
    }

    fn lookup_variable(&self, name: &str, line: usize, column: usize) -> CompilerResult<Type> {
        self.symbol_table
            .get(name)
            .cloned()
            .ok_or_else(|| CompilerError::undefined_variable(name, line, column))
    }

    fn types_compatible(&self, expected: &Type, actual: &Type) -> bool {
        if expected == actual {
            return true;
        }
        // Handle Any type - it's compatible with everything
        if matches!(expected, Type::Any) || matches!(actual, Type::Any) {
            return true;
        }
        // Numeric type promotions
        match (expected, actual) {
            (Type::Number, Type::Integer) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Location;

    #[test]
    fn test_variable_declaration() {
        let mut checker = TypeChecker::new();
        let stmt = Statement::Let {
            name: "x".to_string(),
            type_: Type::Number,
            init: Some(Expr::Number(42.0)),
            location: Location { line: 1, column: 1 },
        };
        assert!(checker.check_statement(&stmt).is_ok());
    }

    #[test]
    fn test_type_mismatch() {
        let mut checker = TypeChecker::new();
        let stmt = Statement::Let {
            name: "x".to_string(),
            type_: Type::Number,
            init: Some(Expr::String("hello".to_string())),
            location: Location { line: 1, column: 1 },
        };
        assert!(checker.check_statement(&stmt).is_err());
    }

    #[test]
    fn test_undefined_variable() {
        let checker = TypeChecker::new();
        let expr = Expr::Variable {
            name: "x".to_string(),
            location: Location { line: 1, column: 1 },
        };
        assert!(checker.infer_type(&expr).is_err());
    }

    #[test]
    fn test_function_call() {
        let mut checker = TypeChecker::new();
        checker.function_table.insert(
            "add".to_string(),
            FunctionType {
                params: vec![Type::Number, Type::Number],
                return_type: Type::Number,
            },
        );

        let expr = Expr::Call {
            function: "add".to_string(),
            args: vec![Expr::Number(1.0), Expr::Number(2.0)],
            location: Location { line: 1, column: 1 },
        };
        assert_eq!(checker.infer_type(&expr).unwrap(), Type::Number);
    }

    #[test]
    fn test_matrix_type_checking() {
        let checker = TypeChecker::new();
        let expr = Expr::Matrix {
            rows: vec![
                vec![Expr::Number(1.0), Expr::Number(2.0)],
                vec![Expr::Number(3.0), Expr::Number(4.0)],
            ],
            location: Location { line: 1, column: 1 },
        };
        assert_eq!(checker.infer_type(&expr).unwrap(), Type::Matrix);

        let expr = Expr::Matrix {
            rows: vec![
                vec![Expr::Number(1.0), Expr::Number(2.0)],
                vec![Expr::Number(3.0)],
            ],
            location: Location { line: 1, column: 1 },
        };
        assert!(checker.infer_type(&expr).is_err());
    }
}