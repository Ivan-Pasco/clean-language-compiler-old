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
        // The imports (math_sin, math_cos, etc.) are registered in codegen_registration.rs
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
        // The imports (math_ln, math_exp, etc.) are registered in codegen_registration.rs
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
        // The imports (math_sinh, math_cosh, math_tanh) are registered in codegen_registration.rs

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
