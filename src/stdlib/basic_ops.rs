use crate::error::CompilerError;
use wasm_encoder::Instruction;

pub mod basic_ops {
    use super::*;

    // Integer operations
    pub fn add_i32(a: i32, b: i32) -> i32 {
        a.wrapping_add(b)
    }

    pub fn sub_i32(a: i32, b: i32) -> i32 {
        a.wrapping_sub(b)
    }

    pub fn mul_i32(a: i32, b: i32) -> i32 {
        a.wrapping_mul(b)
    }

    pub fn div_i32(a: i32, b: i32) -> Result<i32, CompilerError> {
        if b == 0 {
            return Err(CompilerError::type_error(
                "Division by zero",
                Some("Ensure the divisor is not zero".to_string()),
                None,
            ));
        }
        Ok(a.wrapping_div(b))
    }

    // Floating point operations
    pub fn add_f64(a: f64, b: f64) -> f64 {
        a + b
    }

    pub fn sub_f64(a: f64, b: f64) -> f64 {
        a - b
    }

    pub fn mul_f64(a: f64, b: f64) -> f64 {
        a * b
    }

    pub fn div_f64(a: f64, b: f64) -> Result<f64, CompilerError> {
        if b == 0.0 {
            return Err(CompilerError::type_error(
                "Division by zero",
                Some("Ensure the divisor is not zero".to_string()),
                None,
            ));
        }
        Ok(a / b)
    }

    // Comparison operations
    pub fn eq_i32(a: i32, b: i32) -> bool {
        a == b
    }

    pub fn lt_i32(a: i32, b: i32) -> bool {
        a < b
    }

    pub fn gt_i32(a: i32, b: i32) -> bool {
        a > b
    }

    pub fn eq_f64(a: f64, b: f64) -> bool {
        (a - b).abs() < f64::EPSILON
    }

    pub fn lt_f64(a: f64, b: f64) -> bool {
        a < b
    }

    pub fn gt_f64(a: f64, b: f64) -> bool {
        a > b
    }

    pub fn generate_divide_function() -> Result<Vec<Instruction<'static>>, CompilerError> {
        Ok(vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Div,
        ])
    }

    pub fn generate_divide_int_function() -> Result<Vec<Instruction<'static>>, CompilerError> {
        Ok(vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::I32DivS,
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::basic_ops::*;

    #[test]
    fn test_integer_operations() {
        assert_eq!(add_i32(5, 3), 8);
        assert_eq!(sub_i32(5, 3), 2);
        assert_eq!(mul_i32(5, 3), 15);
        assert_eq!(div_i32(6, 2).unwrap(), 3);
        assert!(div_i32(6, 0).is_err());
    }

    #[test]
    fn test_float_operations() {
        assert_eq!(add_f64(5.0, 3.0), 8.0);
        assert_eq!(sub_f64(5.0, 3.0), 2.0);
        assert_eq!(mul_f64(5.0, 3.0), 15.0);
        assert_eq!(div_f64(6.0, 2.0).unwrap(), 3.0);
        assert!(div_f64(6.0, 0.0).is_err());
    }

    #[test]
    fn test_comparisons() {
        assert!(eq_i32(5, 5));
        assert!(lt_i32(3, 5));
        assert!(gt_i32(5, 3));

        assert!(eq_f64(5.0, 5.0));
        assert!(lt_f64(3.0, 5.0));
        assert!(gt_f64(5.0, 3.0));
    }
}
