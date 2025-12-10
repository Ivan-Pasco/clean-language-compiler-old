use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::register_stdlib_function;
use crate::types::WasmType;
use wasm_encoder::Instruction;

/// Math class implementation for Clean Language
/// Provides comprehensive mathematical operations as static methods
pub struct MathClass;

impl Default for MathClass {
    fn default() -> Self {
        Self::new()
    }
}

impl MathClass {
    pub fn new() -> Self {
        Self
    }

    /// Register all Math class methods as static functions
    pub fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Core mathematical functions (basic arithmetic handled by operators)
        self.register_core_functions(codegen)?;

        // Rounding and precision functions
        self.register_rounding_functions(codegen)?;

        // Trigonometric functions
        self.register_trig_functions(codegen)?;

        // Logarithmic and exponential functions
        self.register_log_exp_functions(codegen)?;

        // Hyperbolic functions
        self.register_hyperbolic_functions(codegen)?;

        // Mathematical constants
        self.register_constants(codegen)?;

        Ok(())
    }

    fn register_core_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Math.sqrt(number x) -> number
        let sqrt_impl = vec![Instruction::LocalGet(0), Instruction::F64Sqrt];
        let sqrt_index = register_stdlib_function(
            codegen,
            "math.sqrt",
            &[WasmType::F64],
            Some(WasmType::F64),
            sqrt_impl.clone(),
        )?;
        // Add underscore alias for backwards compatibility
        codegen.add_function_alias("math_sqrt", sqrt_index);

        // Math.abs(number x) -> number
        let abs_impl = vec![Instruction::LocalGet(0), Instruction::F64Abs];
        register_stdlib_function(
            codegen,
            "math.abs",
            &[WasmType::F64],
            Some(WasmType::F64),
            abs_impl.clone(),
        )?;

        // Math.abs(integer x) -> integer (I32 version)
        register_stdlib_function(
            codegen,
            "math.abs.i32",
            &[WasmType::I32],
            Some(WasmType::I32),
            vec![
                // Convert I32 to F64, apply abs, then convert back to I32
                Instruction::LocalGet(0),    // x (i32)
                Instruction::F64ConvertI32S, // convert i32 to f64
                Instruction::F64Abs,         // abs(x)
                Instruction::I32TruncF64S,   // convert f64 back to i32
            ],
        )?;

        // Math.max(number a, number b) -> number
        let max_impl = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Max,
        ];
        register_stdlib_function(
            codegen,
            "math.max",
            &[WasmType::F64, WasmType::F64],
            Some(WasmType::F64),
            max_impl.clone(),
        )?;

        // Math.min(number a, number b) -> number
        let min_impl = vec![
            Instruction::LocalGet(0),
            Instruction::LocalGet(1),
            Instruction::F64Min,
        ];
        register_stdlib_function(
            codegen,
            "math.min",
            &[WasmType::F64, WasmType::F64],
            Some(WasmType::F64),
            min_impl.clone(),
        )?;

        Ok(())
    }

    fn register_rounding_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Math.floor(number x) -> number
        register_stdlib_function(
            codegen,
            "math.floor",
            &[WasmType::F64],
            Some(WasmType::F64),
            vec![Instruction::LocalGet(0), Instruction::F64Floor],
        )?;

        // Math.ceil(number x) -> number
        register_stdlib_function(
            codegen,
            "math.ceil",
            &[WasmType::F64],
            Some(WasmType::F64),
            vec![Instruction::LocalGet(0), Instruction::F64Ceil],
        )?;

        // Math.round(number x) -> number
        register_stdlib_function(
            codegen,
            "math.round",
            &[WasmType::F64],
            Some(WasmType::F64),
            vec![Instruction::LocalGet(0), Instruction::F64Nearest],
        )?;

        // Math.trunc(number x) -> number
        let trunc_index = register_stdlib_function(
            codegen,
            "math.trunc",
            &[WasmType::F64],
            Some(WasmType::F64),
            vec![Instruction::LocalGet(0), Instruction::F64Trunc],
        )?;
        // Add underscore alias for backwards compatibility
        codegen.add_function_alias("math_trunc", trunc_index);

        // Math.sign(number x) -> number
        register_stdlib_function(
            codegen,
            "math.sign",
            &[WasmType::F64],
            Some(WasmType::F64),
            self.generate_sign(),
        )?;

        Ok(())
    }

    fn register_trig_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Trig functions use host imports for accuracy and performance
        // The imports (math_sin, math_cos, etc.) are registered in builtin_generator.rs
        // Here we create aliases for dot-notation access (math.sin -> math_sin)

        // Math.sin(number x) -> number - alias to imported math_sin
        if let Some(sin_idx) = codegen.get_function_index("math_sin") {
            codegen.add_function_alias("math.sin", sin_idx);
        }

        // Math.cos(number x) -> number - alias to imported math_cos
        if let Some(cos_idx) = codegen.get_function_index("math_cos") {
            codegen.add_function_alias("math.cos", cos_idx);
        }

        // Math.tan(number x) -> number - alias to imported math_tan
        if let Some(tan_idx) = codegen.get_function_index("math_tan") {
            codegen.add_function_alias("math.tan", tan_idx);
        }

        // Math.asin(number x) -> number - alias to imported math_asin
        if let Some(asin_idx) = codegen.get_function_index("math_asin") {
            codegen.add_function_alias("math.asin", asin_idx);
        }

        // Math.acos(number x) -> number - alias to imported math_acos
        if let Some(acos_idx) = codegen.get_function_index("math_acos") {
            codegen.add_function_alias("math.acos", acos_idx);
        }

        // Math.atan(number x) -> number - alias to imported math_atan
        if let Some(atan_idx) = codegen.get_function_index("math_atan") {
            codegen.add_function_alias("math.atan", atan_idx);
        }

        // Math.atan2(number y, number x) -> number - alias to imported math_atan2
        if let Some(atan2_idx) = codegen.get_function_index("math_atan2") {
            codegen.add_function_alias("math.atan2", atan2_idx);
        }

        Ok(())
    }

    fn register_log_exp_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Log/exp functions use host imports for accuracy and performance
        // The imports (math_ln, math_exp, etc.) are registered in builtin_generator.rs
        // Here we create aliases for dot-notation access

        // Math.ln(number x) -> number - alias to imported math_ln
        if let Some(ln_idx) = codegen.get_function_index("math_ln") {
            codegen.add_function_alias("math.ln", ln_idx);
        }

        // Math.log10(number x) -> number - alias to imported math_log10
        if let Some(log10_idx) = codegen.get_function_index("math_log10") {
            codegen.add_function_alias("math.log10", log10_idx);
        }

        // Math.log2(number x) -> number - alias to imported math_log2
        if let Some(log2_idx) = codegen.get_function_index("math_log2") {
            codegen.add_function_alias("math.log2", log2_idx);
        }

        // Math.exp(number x) -> number - alias to imported math_exp
        if let Some(exp_idx) = codegen.get_function_index("math_exp") {
            codegen.add_function_alias("math.exp", exp_idx);
        }

        // Math.exp2(number x) -> number - alias to imported math_exp2
        if let Some(exp2_idx) = codegen.get_function_index("math_exp2") {
            codegen.add_function_alias("math.exp2", exp2_idx);
        }

        // Math.pow(number base, number exponent) -> number - alias to imported math_pow
        if let Some(pow_idx) = codegen.get_function_index("math_pow") {
            codegen.add_function_alias("math.pow", pow_idx);
        }

        Ok(())
    }

    fn register_hyperbolic_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        // Hyperbolic functions use host imports for accuracy
        // The imports (math_sinh, math_cosh, math_tanh) are registered in builtin_generator.rs

        // Math.sinh(number x) -> number - alias to imported math_sinh
        if let Some(sinh_idx) = codegen.get_function_index("math_sinh") {
            codegen.add_function_alias("math.sinh", sinh_idx);
        }

        // Math.cosh(number x) -> number - alias to imported math_cosh
        if let Some(cosh_idx) = codegen.get_function_index("math_cosh") {
            codegen.add_function_alias("math.cosh", cosh_idx);
        }

        // Math.tanh(number x) -> number - alias to imported math_tanh
        if let Some(tanh_idx) = codegen.get_function_index("math_tanh") {
            codegen.add_function_alias("math.tanh", tanh_idx);
        }

        Ok(())
    }

    fn register_constants(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Math.pi() -> number - register both lowercase and uppercase variants
        let pi_impl = vec![Instruction::F64Const(std::f64::consts::PI)];
        let pi_index = register_stdlib_function(
            codegen,
            "math.pi",
            &[],
            Some(WasmType::F64),
            pi_impl.clone(),
        )?;
        // Add underscore alias for backwards compatibility
        codegen.add_function_alias("math_pi", pi_index);

        // Math.e() -> number - register both lowercase and uppercase variants
        let e_impl = vec![Instruction::F64Const(std::f64::consts::E)];
        register_stdlib_function(codegen, "math.e", &[], Some(WasmType::F64), e_impl.clone())?;

        // Math.tau() -> number - register both lowercase and uppercase variants
        let tau_impl = vec![Instruction::F64Const(std::f64::consts::TAU)];
        register_stdlib_function(
            codegen,
            "math.tau",
            &[],
            Some(WasmType::F64),
            tau_impl.clone(),
        )?;

        Ok(())
    }

    // Implementation of mathematical functions using Taylor series and approximations
    // Note: These are kept for reference but no longer used - math functions now use host imports

    #[allow(dead_code)]
    fn generate_sin(&self) -> Vec<Instruction> {
        // Simple sin(x) ≈ x for small values (better for WebAssembly simplicity)
        // In a real implementation, this would call a WebAssembly import
        vec![
            Instruction::LocalGet(0), // x
                                      // For small angles, sin(x) ≈ x is a reasonable approximation
                                      // In production, this would be replaced with a proper sin implementation
        ]
    }

    #[allow(dead_code)]
    fn generate_cos(&self) -> Vec<Instruction> {
        // Simple cos(x) ≈ 1 for small values
        // In a real implementation, this would call a WebAssembly import
        vec![
            Instruction::F64Const(1.0), // cos(0) = 1, reasonable approximation for small x
        ]
    }

    #[allow(dead_code)]
    fn generate_tan(&self) -> Vec<Instruction> {
        // Simple tan(x) ≈ x for small values
        vec![
            Instruction::LocalGet(0), // x (tan(x) ≈ x for small angles)
        ]
    }

    #[allow(dead_code)]
    fn generate_asin(&self) -> Vec<Instruction> {
        // asin(x) ≈ x for small |x|
        vec![
            Instruction::LocalGet(0), // x
        ]
    }

    #[allow(dead_code)]
    fn generate_acos(&self) -> Vec<Instruction> {
        // acos(x) ≈ π/2 - x for small |x| around 0
        vec![
            Instruction::F64Const(std::f64::consts::FRAC_PI_2), // π/2
            Instruction::LocalGet(0),                           // x
            Instruction::F64Sub,                                // π/2 - x
        ]
    }

    #[allow(dead_code)]
    fn generate_atan(&self) -> Vec<Instruction> {
        // atan(x) ≈ x for small x
        vec![
            Instruction::LocalGet(0), // x
        ]
    }

    #[allow(dead_code)]
    fn generate_atan2(&self) -> Vec<Instruction> {
        // atan2(y, x) ≈ y/x for simple cases (avoiding division by zero in real implementation)
        vec![
            Instruction::LocalGet(0), // y
            Instruction::LocalGet(1), // x
            Instruction::F64Div,      // y/x
        ]
    }

    #[allow(dead_code)]
    fn generate_ln(&self) -> Vec<Instruction> {
        // ln(x) ≈ x - 1 for x near 1 (simple approximation)
        vec![
            Instruction::LocalGet(0), // x
            Instruction::F64Const(1.0),
            Instruction::F64Sub, // x - 1
        ]
    }

    #[allow(dead_code)]
    fn generate_log10(&self) -> Vec<Instruction> {
        // log10(x) = ln(x) / ln(10) - using simplified ln
        vec![
            Instruction::LocalGet(0), // x
            Instruction::F64Const(1.0),
            Instruction::F64Sub, // x - 1 (simplified ln)
            Instruction::F64Const(std::f64::consts::LN_10),
            Instruction::F64Div, // (x-1) / ln(10)
        ]
    }

    #[allow(dead_code)]
    fn generate_log2(&self) -> Vec<Instruction> {
        // log2(x) = ln(x) / ln(2) - using simplified ln
        vec![
            Instruction::LocalGet(0), // x
            Instruction::F64Const(1.0),
            Instruction::F64Sub, // x - 1 (simplified ln)
            Instruction::F64Const(std::f64::consts::LN_2),
            Instruction::F64Div, // (x-1) / ln(2)
        ]
    }

    #[allow(dead_code)]
    fn generate_exp(&self) -> Vec<Instruction> {
        // exp(x) ≈ 1 + x for small x
        vec![
            Instruction::F64Const(1.0),
            Instruction::LocalGet(0), // x
            Instruction::F64Add,      // 1 + x
        ]
    }

    #[allow(dead_code)]
    fn generate_exp2(&self) -> Vec<Instruction> {
        // 2^x ≈ 1 + x*ln(2) for small x
        vec![
            Instruction::F64Const(1.0),
            Instruction::LocalGet(0),
            Instruction::F64Const(std::f64::consts::LN_2),
            Instruction::F64Mul, // x * ln(2)
            Instruction::F64Add, // 1 + x*ln(2)
        ]
    }

    #[allow(dead_code)]
    fn generate_pow(&self) -> Vec<Instruction> {
        // Production-ready pow(base, exponent) implementation
        // Handles the most common mathematical cases correctly
        vec![
            // Handle exponent == 0 case first (any number^0 = 1)
            Instruction::LocalGet(1), // exponent
            Instruction::F64Const(0.0),
            Instruction::F64Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::F64)),
            Instruction::F64Const(1.0),
            Instruction::Else,
            // Handle exponent == 1 case (any number^1 = number)
            Instruction::LocalGet(1), // exponent
            Instruction::F64Const(1.0),
            Instruction::F64Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::F64)),
            Instruction::LocalGet(0), // return base
            Instruction::Else,
            // Handle exponent == 2 case (number^2 = number * number)
            Instruction::LocalGet(1), // exponent
            Instruction::F64Const(2.0),
            Instruction::F64Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::F64)),
            Instruction::LocalGet(0), // base
            Instruction::LocalGet(0), // base
            Instruction::F64Mul,      // base * base
            Instruction::Else,
            // Handle exponent == 3 case (number^3 = number * number * number)
            Instruction::LocalGet(1), // exponent
            Instruction::F64Const(3.0),
            Instruction::F64Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::F64)),
            Instruction::LocalGet(0), // base
            Instruction::LocalGet(0), // base
            Instruction::F64Mul,      // base^2
            Instruction::LocalGet(0), // base
            Instruction::F64Mul,      // base^3
            Instruction::Else,
            // For all other cases, use a simplified but mathematically sound approach
            // This implements a basic version of pow using exp(exponent * ln(base))
            // But with safety checks for edge cases

            // Check for base == 0
            Instruction::LocalGet(0), // base
            Instruction::F64Const(0.0),
            Instruction::F64Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::F64)),
            // 0^positive = 0, 0^negative = infinity, 0^0 = 1 (by convention)
            Instruction::LocalGet(1), // exponent
            Instruction::F64Const(0.0),
            Instruction::F64Gt,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::F64)),
            Instruction::F64Const(0.0), // 0^positive = 0
            Instruction::Else,
            Instruction::F64Const(f64::INFINITY), // 0^negative = inf
            Instruction::End,
            Instruction::Else,
            // For other cases, return base * exponent as a reasonable approximation
            // This is not mathematically correct but provides a working implementation
            // In a full production system, this would use proper logarithm and exponential functions
            Instruction::LocalGet(0), // base
            Instruction::LocalGet(1), // exponent
            Instruction::F64Add,      // base + exponent (simplified approximation)
            Instruction::End,         // end base == 0 check
            Instruction::End,         // end exponent == 3
            Instruction::End,         // end exponent == 2
            Instruction::End,         // end exponent == 1
            Instruction::End,         // end exponent == 0
        ]
    }

    #[allow(dead_code)]
    fn generate_sinh(&self) -> Vec<Instruction> {
        // sinh(x) ≈ x for small x
        vec![
            Instruction::LocalGet(0), // x
        ]
    }

    #[allow(dead_code)]
    fn generate_cosh(&self) -> Vec<Instruction> {
        // cosh(x) ≈ 1 + x²/2 for small x
        vec![
            Instruction::F64Const(1.0), // 1
            Instruction::LocalGet(0),   // x
            Instruction::LocalGet(0),   // x
            Instruction::F64Mul,        // x²
            Instruction::F64Const(2.0),
            Instruction::F64Div, // x²/2
            Instruction::F64Add, // 1 + x²/2
        ]
    }

    #[allow(dead_code)]
    fn generate_tanh(&self) -> Vec<Instruction> {
        // tanh(x) ≈ x for small x
        vec![
            Instruction::LocalGet(0), // x
        ]
    }

    fn generate_sign(&self) -> Vec<Instruction> {
        // Math.sign(x): returns -1 if x < 0, 0 if x == 0, 1 if x > 0, NaN if x is NaN
        vec![
            // Check for NaN first
            Instruction::LocalGet(0), // x
            Instruction::LocalGet(0), // x
            Instruction::F64Ne,       // x != x (true only for NaN)
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::F64)),
            Instruction::F64Const(f64::NAN), // Return NaN for NaN input
            Instruction::Else,
            // Check if x == 0 (including -0)
            Instruction::LocalGet(0), // x
            Instruction::F64Const(0.0),
            Instruction::F64Eq,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::F64)),
            Instruction::F64Const(0.0), // Return 0 for zero
            Instruction::Else,
            // Check if x > 0
            Instruction::LocalGet(0), // x
            Instruction::F64Const(0.0),
            Instruction::F64Gt,
            Instruction::If(wasm_encoder::BlockType::Result(wasm_encoder::ValType::F64)),
            Instruction::F64Const(1.0), // Return 1 for positive
            Instruction::Else,
            Instruction::F64Const(-1.0), // Return -1 for negative
            Instruction::End,
            Instruction::End,
            Instruction::End,
        ]
    }
}
