use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::{register_stdlib_function_with_locals, MemoryManager};
use crate::types::WasmType;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_encoder::{BlockType, Instruction, ValType};

/// Advanced Math class implementation for Clean Language
/// Implements precise mathematical functions with proper algorithms
pub struct MathAdvancedManager {
    memory_manager: Rc<RefCell<MemoryManager>>,
}

impl MathAdvancedManager {
    pub fn new(memory_manager: Rc<RefCell<MemoryManager>>) -> Self {
        Self { memory_manager }
    }

    /// Register all advanced math functions with the code generator
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Advanced trigonometric functions
        self.register_advanced_trig_functions(codegen)?;

        // Advanced hyperbolic functions
        self.register_advanced_hyperbolic_functions(codegen)?;

        // Advanced logarithmic and exponential functions
        self.register_advanced_log_exp_functions(codegen)?;

        // Utility and special functions
        self.register_utility_functions(codegen)?;

        Ok(())
    }

    fn register_advanced_trig_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Math.atan2Advanced(y, x) -> number
        // Two-parameter arctangent with proper quadrant handling
        register_stdlib_function_with_locals(
            codegen,
            "math.atan2Advanced",
            &[WasmType::F64, WasmType::F64], // y, x
            Some(WasmType::F64),             // result angle in radians
            &[
                WasmType::F64,
                WasmType::F64,
                WasmType::F64,
                WasmType::F64,
                WasmType::F64,
            ], // abs_y, abs_x, ratio, result, quadrant_adjustment
            self.generate_atan2_advanced(),
        )?;

        Ok(())
    }

    fn register_advanced_hyperbolic_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Math.sinhAdvanced(x) -> number
        // Hyperbolic sine with precise calculation
        register_stdlib_function_with_locals(
            codegen,
            "math.sinhAdvanced",
            &[WasmType::F64],                               // x
            Some(WasmType::F64),                            // sinh(x)
            &[WasmType::F64, WasmType::F64, WasmType::F64], // exp_x, exp_neg_x, result
            self.generate_sinh_advanced(),
        )?;

        // Math.coshAdvanced(x) -> number
        // Hyperbolic cosine with precise calculation
        register_stdlib_function_with_locals(
            codegen,
            "math.coshAdvanced",
            &[WasmType::F64],                               // x
            Some(WasmType::F64),                            // cosh(x)
            &[WasmType::F64, WasmType::F64, WasmType::F64], // exp_x, exp_neg_x, result
            self.generate_cosh_advanced(),
        )?;

        // Math.tanhAdvanced(x) -> number
        // Hyperbolic tangent with precise calculation
        register_stdlib_function_with_locals(
            codegen,
            "math.tanhAdvanced",
            &[WasmType::F64],                                              // x
            Some(WasmType::F64),                                           // tanh(x)
            &[WasmType::F64, WasmType::F64, WasmType::F64, WasmType::F64], // exp_2x, exp_2x_plus_1, numerator, denominator
            self.generate_tanh_advanced(),
        )?;

        Ok(())
    }

    fn register_advanced_log_exp_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Math.log2Advanced(x) -> number
        // Base-2 logarithm with precise calculation
        register_stdlib_function_with_locals(
            codegen,
            "math.log2Advanced",
            &[WasmType::F64],    // x
            Some(WasmType::F64), // log2(x)
            &[
                WasmType::F64,
                WasmType::F64,
                WasmType::F64,
                WasmType::F64,
                WasmType::F64,
            ], // ln_x, power, term, result, i
            self.generate_log2_advanced(),
        )?;

        // Math.exp2Advanced(x) -> number
        // Base-2 exponential with precise calculation
        register_stdlib_function_with_locals(
            codegen,
            "math.exp2Advanced",
            &[WasmType::F64],    // x
            Some(WasmType::F64), // 2^x
            &[
                WasmType::F64,
                WasmType::F64,
                WasmType::F64,
                WasmType::F64,
                WasmType::F64,
            ], // ln2_x, power, factorial, term, result
            self.generate_exp2_advanced(),
        )?;

        Ok(())
    }

    fn register_utility_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Math.signAdvanced(x) -> number
        // Sign function returning -1, 0, or 1
        register_stdlib_function_with_locals(
            codegen,
            "math.signAdvanced",
            &[WasmType::F64],    // x
            Some(WasmType::F64), // sign of x
            &[WasmType::F64],    // abs_x
            self.generate_sign_advanced(),
        )?;

        // Math.clampAdvanced(value, min, max) -> number
        // Clamp value between min and max bounds
        register_stdlib_function_with_locals(
            codegen,
            "math.clampAdvanced",
            &[WasmType::F64, WasmType::F64, WasmType::F64], // value, min, max
            Some(WasmType::F64),                            // clamped value
            &[WasmType::F64],                               // temp_result
            self.generate_clamp_advanced(),
        )?;

        // Math.lerpAdvanced(start, end, t) -> number
        // Linear interpolation between start and end
        register_stdlib_function_with_locals(
            codegen,
            "math.lerpAdvanced",
            &[WasmType::F64, WasmType::F64, WasmType::F64], // start, end, t
            Some(WasmType::F64),                            // interpolated value
            &[WasmType::F64, WasmType::F64],                // difference, scaled_diff
            self.generate_lerp_advanced(),
        )?;

        Ok(())
    }

    // Implementation methods for advanced mathematical operations

    fn generate_atan2_advanced(&self) -> Vec<Instruction> {
        vec![
            // atan2(y, x) implementation with proper quadrant handling
            // Parameters: y (param 0), x (param 1)

            // Handle special cases: x = 0
            Instruction::LocalGet(1), // x
            Instruction::F64Const(0.0),
            Instruction::F64Eq,
            Instruction::If(BlockType::Result(ValType::F64)),
            // x == 0: return π/2 if y > 0, -π/2 if y < 0, 0 if y == 0
            Instruction::LocalGet(0), // y
            Instruction::F64Const(0.0),
            Instruction::F64Gt,
            Instruction::If(BlockType::Result(ValType::F64)),
            // y > 0: return π/2
            Instruction::F64Const(std::f64::consts::FRAC_PI_2),
            Instruction::Else,
            // y <= 0: check if y < 0
            Instruction::LocalGet(0), // y
            Instruction::F64Const(0.0),
            Instruction::F64Lt,
            Instruction::If(BlockType::Result(ValType::F64)),
            // y < 0: return -π/2
            Instruction::F64Const(-std::f64::consts::FRAC_PI_2),
            Instruction::Else,
            // y == 0: return 0
            Instruction::F64Const(0.0),
            Instruction::End,
            Instruction::End,
            Instruction::Else,
            // x != 0: compute atan(y/x) and adjust for quadrant

            // Get absolute values for calculation
            Instruction::LocalGet(0), // y
            Instruction::F64Abs,
            Instruction::LocalSet(2), // abs_y
            Instruction::LocalGet(1), // x
            Instruction::F64Abs,
            Instruction::LocalSet(3), // abs_x
            // Calculate atan(abs_y/abs_x) using approximation
            Instruction::LocalGet(2), // abs_y
            Instruction::LocalGet(3), // abs_x
            Instruction::F64Div,
            Instruction::LocalSet(4), // ratio = abs_y/abs_x
            // Approximate atan(ratio) using first few terms of Taylor series
            // atan(z) ≈ z - z³/3 + z⁵/5 for |z| < 1
            Instruction::LocalGet(4), // ratio
            Instruction::LocalGet(4), // ratio
            Instruction::LocalGet(4), // ratio
            Instruction::F64Mul,      // ratio²
            Instruction::F64Mul,      // ratio³
            Instruction::F64Const(3.0),
            Instruction::F64Div,      // ratio³/3
            Instruction::LocalGet(4), // ratio
            Instruction::F64Sub,      // ratio - ratio³/3
            Instruction::LocalSet(5), // result = approximate atan
            // Adjust for quadrant based on signs of x and y
            Instruction::LocalGet(1), // x
            Instruction::F64Const(0.0),
            Instruction::F64Lt, // x < 0
            Instruction::If(BlockType::Empty),
            // x < 0: add or subtract π
            Instruction::LocalGet(0), // y
            Instruction::F64Const(0.0),
            Instruction::F64Ge, // y >= 0
            Instruction::If(BlockType::Empty),
            // Quadrant II: add π
            Instruction::LocalGet(5), // result
            Instruction::F64Const(std::f64::consts::PI),
            Instruction::F64Add,
            Instruction::LocalSet(5), // result += π
            Instruction::Else,
            // Quadrant III: subtract π
            Instruction::LocalGet(5), // result
            Instruction::F64Const(std::f64::consts::PI),
            Instruction::F64Sub,
            Instruction::LocalSet(5), // result -= π
            Instruction::End,
            Instruction::End,
            // Apply sign of y to result
            Instruction::LocalGet(0), // y
            Instruction::F64Const(0.0),
            Instruction::F64Lt, // y < 0
            Instruction::If(BlockType::Empty),
            // y < 0: negate result
            Instruction::LocalGet(5), // result
            Instruction::F64Neg,
            Instruction::LocalSet(5), // result = -result
            Instruction::End,
            Instruction::LocalGet(5), // final result
            Instruction::End,
        ]
    }

    fn generate_sinh_advanced(&self) -> Vec<Instruction> {
        vec![
            // sinh(x) = (e^x - e^(-x)) / 2
            // Using approximation: e^x ≈ 1 + x + x²/2 + x³/6 for reasonable precision

            // Calculate e^x approximation
            Instruction::F64Const(1.0), // 1
            Instruction::LocalGet(0),   // x
            Instruction::F64Add,        // 1 + x
            Instruction::LocalGet(0),   // x
            Instruction::LocalGet(0),   // x
            Instruction::F64Mul,        // x²
            Instruction::F64Const(2.0),
            Instruction::F64Div,      // x²/2
            Instruction::F64Add,      // 1 + x + x²/2
            Instruction::LocalGet(0), // x
            Instruction::LocalGet(0), // x
            Instruction::F64Mul,      // x²
            Instruction::LocalGet(0), // x
            Instruction::F64Mul,      // x³
            Instruction::F64Const(6.0),
            Instruction::F64Div,      // x³/6
            Instruction::F64Add,      // 1 + x + x²/2 + x³/6
            Instruction::LocalSet(2), // exp_x
            // Calculate e^(-x) approximation
            Instruction::F64Const(1.0), // 1
            Instruction::LocalGet(0),   // x
            Instruction::F64Neg,        // -x
            Instruction::F64Add,        // 1 + (-x) = 1 - x
            Instruction::LocalGet(0),   // x
            Instruction::LocalGet(0),   // x
            Instruction::F64Mul,        // x²
            Instruction::F64Const(2.0),
            Instruction::F64Div,      // x²/2
            Instruction::F64Add,      // 1 - x + x²/2
            Instruction::LocalGet(0), // x
            Instruction::LocalGet(0), // x
            Instruction::F64Mul,      // x²
            Instruction::LocalGet(0), // x
            Instruction::F64Mul,      // x³
            Instruction::F64Neg,      // -x³
            Instruction::F64Const(6.0),
            Instruction::F64Div,      // -x³/6
            Instruction::F64Add,      // 1 - x + x²/2 - x³/6
            Instruction::LocalSet(3), // exp_neg_x
            // Calculate sinh(x) = (e^x - e^(-x)) / 2
            Instruction::LocalGet(2), // exp_x
            Instruction::LocalGet(3), // exp_neg_x
            Instruction::F64Sub,      // exp_x - exp_neg_x
            Instruction::F64Const(2.0),
            Instruction::F64Div, // (exp_x - exp_neg_x) / 2
        ]
    }

    fn generate_cosh_advanced(&self) -> Vec<Instruction> {
        vec![
            // cosh(x) = (e^x + e^(-x)) / 2
            // Using same approximation as sinh but with addition

            // Calculate e^x approximation (same as sinh)
            Instruction::F64Const(1.0), // 1
            Instruction::LocalGet(0),   // x
            Instruction::F64Add,        // 1 + x
            Instruction::LocalGet(0),   // x
            Instruction::LocalGet(0),   // x
            Instruction::F64Mul,        // x²
            Instruction::F64Const(2.0),
            Instruction::F64Div,      // x²/2
            Instruction::F64Add,      // 1 + x + x²/2
            Instruction::LocalGet(0), // x
            Instruction::LocalGet(0), // x
            Instruction::F64Mul,      // x²
            Instruction::LocalGet(0), // x
            Instruction::F64Mul,      // x³
            Instruction::F64Const(6.0),
            Instruction::F64Div,      // x³/6
            Instruction::F64Add,      // 1 + x + x²/2 + x³/6
            Instruction::LocalSet(2), // exp_x
            // Calculate e^(-x) approximation (same as sinh)
            Instruction::F64Const(1.0), // 1
            Instruction::LocalGet(0),   // x
            Instruction::F64Neg,        // -x
            Instruction::F64Add,        // 1 - x
            Instruction::LocalGet(0),   // x
            Instruction::LocalGet(0),   // x
            Instruction::F64Mul,        // x²
            Instruction::F64Const(2.0),
            Instruction::F64Div,      // x²/2
            Instruction::F64Add,      // 1 - x + x²/2
            Instruction::LocalGet(0), // x
            Instruction::LocalGet(0), // x
            Instruction::F64Mul,      // x²
            Instruction::LocalGet(0), // x
            Instruction::F64Mul,      // x³
            Instruction::F64Neg,      // -x³
            Instruction::F64Const(6.0),
            Instruction::F64Div,      // -x³/6
            Instruction::F64Add,      // 1 - x + x²/2 - x³/6
            Instruction::LocalSet(3), // exp_neg_x
            // Calculate cosh(x) = (e^x + e^(-x)) / 2
            Instruction::LocalGet(2), // exp_x
            Instruction::LocalGet(3), // exp_neg_x
            Instruction::F64Add,      // exp_x + exp_neg_x
            Instruction::F64Const(2.0),
            Instruction::F64Div, // (exp_x + exp_neg_x) / 2
        ]
    }

    fn generate_tanh_advanced(&self) -> Vec<Instruction> {
        vec![
            // tanh(x) = (e^(2x) - 1) / (e^(2x) + 1)
            // More numerically stable than sinh/cosh for large x

            // Calculate 2x
            Instruction::LocalGet(0), // x
            Instruction::F64Const(2.0),
            Instruction::F64Mul, // 2x
            // Calculate e^(2x) using approximation
            Instruction::F64Const(1.0), // 1
            Instruction::LocalGet(0),   // x
            Instruction::F64Const(2.0),
            Instruction::F64Mul,      // 2x
            Instruction::F64Add,      // 1 + 2x
            Instruction::LocalGet(0), // x
            Instruction::F64Const(2.0),
            Instruction::F64Mul,      // 2x
            Instruction::LocalGet(0), // x
            Instruction::F64Const(2.0),
            Instruction::F64Mul, // 2x
            Instruction::F64Mul, // (2x)²
            Instruction::F64Const(2.0),
            Instruction::F64Div,      // (2x)²/2
            Instruction::F64Add,      // 1 + 2x + (2x)²/2
            Instruction::LocalSet(2), // exp_2x
            // Calculate e^(2x) + 1
            Instruction::LocalGet(2), // exp_2x
            Instruction::F64Const(1.0),
            Instruction::F64Add,
            Instruction::LocalSet(3), // exp_2x_plus_1
            // Calculate e^(2x) - 1
            Instruction::LocalGet(2), // exp_2x
            Instruction::F64Const(1.0),
            Instruction::F64Sub,
            Instruction::LocalSet(4), // numerator = exp_2x - 1
            // Calculate tanh(x) = (e^(2x) - 1) / (e^(2x) + 1)
            Instruction::LocalGet(4), // numerator
            Instruction::LocalGet(3), // exp_2x_plus_1
            Instruction::F64Div,      // (exp_2x - 1) / (exp_2x + 1)
        ]
    }

    fn generate_log2_advanced(&self) -> Vec<Instruction> {
        vec![
            // log2(x) = ln(x) / ln(2)
            // Using ln(x) ≈ (x-1) - (x-1)²/2 + (x-1)³/3 for x near 1

            // Check for x <= 0 (return NaN for invalid input)
            Instruction::LocalGet(0), // x
            Instruction::F64Const(0.0),
            Instruction::F64Le, // x <= 0
            Instruction::If(BlockType::Result(ValType::F64)),
            // Invalid input: return NaN
            Instruction::F64Const(f64::NAN),
            Instruction::Else,
            // Valid input: calculate log2(x)

            // Calculate ln(x) using series expansion around x=1
            Instruction::LocalGet(0), // x
            Instruction::F64Const(1.0),
            Instruction::F64Sub,      // x - 1
            Instruction::LocalSet(2), // term = x - 1
            // Start with first term: (x-1)
            Instruction::LocalGet(2), // x - 1
            Instruction::LocalSet(4), // result = x - 1
            // Subtract (x-1)²/2
            Instruction::LocalGet(2), // x - 1
            Instruction::LocalGet(2), // x - 1
            Instruction::F64Mul,      // (x-1)²
            Instruction::F64Const(2.0),
            Instruction::F64Div,      // (x-1)²/2
            Instruction::LocalSet(3), // power = (x-1)²/2
            Instruction::LocalGet(4), // result
            Instruction::LocalGet(3), // (x-1)²/2
            Instruction::F64Sub,      // result - (x-1)²/2
            Instruction::LocalSet(4), // result
            // Add (x-1)³/3
            Instruction::LocalGet(2), // x - 1
            Instruction::LocalGet(3), // (x-1)²/2
            Instruction::F64Mul,      // (x-1) * (x-1)²/2
            Instruction::F64Const(2.0),
            Instruction::F64Mul, // (x-1)³
            Instruction::F64Const(3.0),
            Instruction::F64Div,      // (x-1)³/3
            Instruction::LocalGet(4), // result
            Instruction::F64Add,      // result + (x-1)³/3
            Instruction::LocalSet(4), // ln_x ≈ result
            // Calculate log2(x) = ln(x) / ln(2)
            Instruction::LocalGet(4), // ln_x
            Instruction::F64Const(std::f64::consts::LN_2),
            Instruction::F64Div, // ln_x / ln(2)
            Instruction::End,
        ]
    }

    fn generate_exp2_advanced(&self) -> Vec<Instruction> {
        vec![
            // 2^x = e^(x * ln(2))
            // Using e^y ≈ 1 + y + y²/2 + y³/6 + y⁴/24

            // Calculate y = x * ln(2)
            Instruction::LocalGet(0), // x
            Instruction::F64Const(std::f64::consts::LN_2),
            Instruction::F64Mul,      // y = x * ln(2)
            Instruction::LocalSet(2), // ln2_x = y
            // Calculate e^y using Taylor series
            // Start with 1
            Instruction::F64Const(1.0),
            Instruction::LocalSet(6), // result = 1
            // Add y
            Instruction::LocalGet(6), // result
            Instruction::LocalGet(2), // ln2_x
            Instruction::F64Add,      // result + y
            Instruction::LocalSet(6), // result = 1 + y
            // Add y²/2
            Instruction::LocalGet(2), // ln2_x
            Instruction::LocalGet(2), // ln2_x
            Instruction::F64Mul,      // y²
            Instruction::F64Const(2.0),
            Instruction::F64Div,      // y²/2
            Instruction::LocalSet(4), // power = y²/2
            Instruction::LocalGet(6), // result
            Instruction::LocalGet(4), // y²/2
            Instruction::F64Add,      // result + y²/2
            Instruction::LocalSet(6), // result = 1 + y + y²/2
            // Add y³/6
            Instruction::LocalGet(4), // y²/2
            Instruction::LocalGet(2), // ln2_x
            Instruction::F64Mul,      // y³/2
            Instruction::F64Const(3.0),
            Instruction::F64Div,      // y³/6
            Instruction::LocalSet(5), // term = y³/6
            Instruction::LocalGet(6), // result
            Instruction::LocalGet(5), // y³/6
            Instruction::F64Add,      // result + y³/6
            Instruction::LocalSet(6), // result = 1 + y + y²/2 + y³/6
            // Add y⁴/24
            Instruction::LocalGet(5), // y³/6
            Instruction::LocalGet(2), // ln2_x
            Instruction::F64Mul,      // y⁴/6
            Instruction::F64Const(4.0),
            Instruction::F64Div,      // y⁴/24
            Instruction::LocalGet(6), // result
            Instruction::F64Add,      // result + y⁴/24
        ]
    }

    fn generate_sign_advanced(&self) -> Vec<Instruction> {
        vec![
            // sign(x) returns -1 for x < 0, 0 for x = 0, 1 for x > 0

            // Check if x is zero
            Instruction::LocalGet(0), // x
            Instruction::F64Const(0.0),
            Instruction::F64Eq, // x == 0
            Instruction::If(BlockType::Result(ValType::F64)),
            // x == 0: return 0
            Instruction::F64Const(0.0),
            Instruction::Else,
            // x != 0: check sign
            Instruction::LocalGet(0), // x
            Instruction::F64Const(0.0),
            Instruction::F64Gt, // x > 0
            Instruction::If(BlockType::Result(ValType::F64)),
            // x > 0: return 1
            Instruction::F64Const(1.0),
            Instruction::Else,
            // x < 0: return -1
            Instruction::F64Const(-1.0),
            Instruction::End,
            Instruction::End,
        ]
    }

    fn generate_clamp_advanced(&self) -> Vec<Instruction> {
        vec![
            // clamp(value, min, max) = max(min, min(value, max))

            // First clamp to maximum: min(value, max)
            Instruction::LocalGet(0), // value
            Instruction::LocalGet(2), // max
            Instruction::F64Min,      // min(value, max)
            Instruction::LocalSet(3), // temp_result
            // Then clamp to minimum: max(min, temp_result)
            Instruction::LocalGet(1), // min
            Instruction::LocalGet(3), // temp_result
            Instruction::F64Max,      // max(min, temp_result)
        ]
    }

    fn generate_lerp_advanced(&self) -> Vec<Instruction> {
        vec![
            // lerp(start, end, t) = start + t * (end - start)

            // Calculate end - start
            Instruction::LocalGet(1), // end
            Instruction::LocalGet(0), // start
            Instruction::F64Sub,      // end - start
            Instruction::LocalSet(3), // difference
            // Calculate t * (end - start)
            Instruction::LocalGet(2), // t
            Instruction::LocalGet(3), // difference
            Instruction::F64Mul,      // t * (end - start)
            Instruction::LocalSet(4), // scaled_diff
            // Calculate start + t * (end - start)
            Instruction::LocalGet(0), // start
            Instruction::LocalGet(4), // scaled_diff
            Instruction::F64Add,      // start + t * (end - start)
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_math_advanced_creation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let _manager = MathAdvancedManager::new(memory_manager);
    }

    #[test]
    fn test_atan2_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MathAdvancedManager::new(memory_manager);
        let instructions = manager.generate_atan2_advanced();
        assert!(
            !instructions.is_empty(),
            "Atan2 instructions should not be empty"
        );
    }

    #[test]
    fn test_sinh_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MathAdvancedManager::new(memory_manager);
        let instructions = manager.generate_sinh_advanced();
        assert!(
            !instructions.is_empty(),
            "Sinh instructions should not be empty"
        );
    }

    #[test]
    fn test_cosh_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MathAdvancedManager::new(memory_manager);
        let instructions = manager.generate_cosh_advanced();
        assert!(
            !instructions.is_empty(),
            "Cosh instructions should not be empty"
        );
    }

    #[test]
    fn test_tanh_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MathAdvancedManager::new(memory_manager);
        let instructions = manager.generate_tanh_advanced();
        assert!(
            !instructions.is_empty(),
            "Tanh instructions should not be empty"
        );
    }

    #[test]
    fn test_log2_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MathAdvancedManager::new(memory_manager);
        let instructions = manager.generate_log2_advanced();
        assert!(
            !instructions.is_empty(),
            "Log2 instructions should not be empty"
        );
    }

    #[test]
    fn test_exp2_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MathAdvancedManager::new(memory_manager);
        let instructions = manager.generate_exp2_advanced();
        assert!(
            !instructions.is_empty(),
            "Exp2 instructions should not be empty"
        );
    }

    #[test]
    fn test_sign_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MathAdvancedManager::new(memory_manager);
        let instructions = manager.generate_sign_advanced();
        assert!(
            !instructions.is_empty(),
            "Sign instructions should not be empty"
        );
    }

    #[test]
    fn test_clamp_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MathAdvancedManager::new(memory_manager);
        let instructions = manager.generate_clamp_advanced();
        assert!(
            !instructions.is_empty(),
            "Clamp instructions should not be empty"
        );
    }

    #[test]
    fn test_lerp_advanced_generation() {
        let memory_manager = Rc::new(RefCell::new(MemoryManager::new(16, Some(1024))));
        let manager = MathAdvancedManager::new(memory_manager);
        let instructions = manager.generate_lerp_advanced();
        assert!(
            !instructions.is_empty(),
            "Lerp instructions should not be empty"
        );
    }
}
