use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::{register_stdlib_function, register_stdlib_function_with_locals};
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

        // `math.abs(integer)` and the integer powers `^` calls raise
        // "integer overflow", so they are registered after the error
        // handling runtime (`register_checked_integer_functions`).

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

        // math.max / math.min of two integers -> integer
        for (name, keep_first) in [
            ("math.max.i32", Instruction::I32GtS),
            ("math.min.i32", Instruction::I32LtS),
        ] {
            register_stdlib_function(
                codegen,
                name,
                &[WasmType::I32, WasmType::I32],
                Some(WasmType::I32),
                vec![
                    Instruction::LocalGet(0),
                    Instruction::LocalGet(1),
                    Instruction::LocalGet(0),
                    Instruction::LocalGet(1),
                    keep_first,
                    Instruction::Select,
                ],
            )?;
        }

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
            // Math.log(x) is natural log — same host function as math.ln
            codegen.add_function_alias("math.log", ln_idx);
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

        // Math.random() -> number — alias to imported math_random (returns [0.0, 1.0))
        if let Some(random_idx) = codegen.get_function_index("math_random") {
            codegen.add_function_alias("math.random", random_idx);
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

    /// The integer helpers that raise "integer overflow": `math.abs.i32`
    /// (`math.abs(integer)`) and the exact integer powers `math.pow.i32`,
    /// `math.pow.u32`, `math.pow.i64` and `math.pow.u64` (the MIR builder
    /// calls these for `^` on integer operands).
    ///
    /// They raise through `__err_raise`, so they are registered once the
    /// error-handling runtime exists (`register_error_handling_operations`).
    pub fn register_checked_integer_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        let err_raise = codegen.get_function_index("__err_raise").ok_or_else(|| {
            CompilerError::codegen_error(
                "checked integer math requires __err_raise, which was not registered".to_string(),
                None,
                None,
            )
        })?;
        let overflow = codegen.get_or_create_string_offset("integer overflow")? as i32;
        let division_by_zero = codegen.get_or_create_string_offset("division by zero")? as i32;

        // math.abs(integer x) -> integer. The absolute value of the smallest
        // integer is outside integer: "integer overflow", as `-x` raises.
        register_stdlib_function(
            codegen,
            "math.abs.i32",
            &[WasmType::I32],
            Some(WasmType::I32),
            vec![
                Instruction::LocalGet(0),
                Instruction::I32Const(i32::MIN),
                Instruction::I32Eq,
                Instruction::If(wasm_encoder::BlockType::Empty),
                Instruction::I32Const(overflow),
                Instruction::Call(err_raise),
                Instruction::I32Const(0),
                Instruction::Return,
                Instruction::End,
                Instruction::I32Const(0),
                Instruction::LocalGet(0),
                Instruction::I32Sub, // -x
                Instruction::LocalGet(0),
                Instruction::LocalGet(0),
                Instruction::I32Const(0),
                Instruction::I32LtS,
                Instruction::Select, // x < 0 ? -x : x
            ],
        )?;

        for (name, wide, unsigned) in [
            ("math.pow.i32", false, false),
            ("math.pow.u32", false, true),
            ("math.pow.i64", true, false),
            ("math.pow.u64", true, true),
        ] {
            let ty = if wide { WasmType::I64 } else { WasmType::I32 };
            register_stdlib_function_with_locals(
                codegen,
                name,
                &[ty, ty],
                Some(ty),
                // 2: result, 3: the 64-bit product
                &[ty, WasmType::I64],
                Self::generate_integer_pow(wide, unsigned, overflow, division_by_zero, err_raise),
            )?;
        }
        Ok(())
    }

    /// `base ^ exponent` on i32 (`wide == false`) or i64 operands, exact:
    /// a result outside the type (`unsigned`: `0..=MAX`) raises "integer
    /// overflow" and gives 0. A negative exponent (signed types) gives the
    /// integer part of `1 / base^-exponent`: 1 for base 1, ±1 for base -1, 0
    /// otherwise, and "division by zero" for base 0, as `1 / 0` does.
    ///
    /// The bases 0, 1 and -1 have closed forms. Any other base leaves the
    /// type within 64 multiplications, so the loop multiplies once per
    /// exponent step and stops at the first overflow. 32-bit products are
    /// formed in i64; i64 products are checked by dividing back (the running
    /// result is never 0 or -1, so the division cannot trap).
    ///
    /// Locals: 0 base, 1 exponent (parameters), 2 result, 3 product.
    fn generate_integer_pow(
        wide: bool,
        unsigned: bool,
        overflow: i32,
        division_by_zero: i32,
        err_raise: u32,
    ) -> Vec<Instruction<'static>> {
        use wasm_encoder::BlockType;
        let c = |v: i64| {
            if wide {
                Instruction::I64Const(v)
            } else {
                Instruction::I32Const(v as i32)
            }
        };
        let pick = |narrow: Instruction<'static>, wide_op: Instruction<'static>| {
            if wide {
                wide_op
            } else {
                narrow
            }
        };
        let raise = |message: i32| {
            vec![
                Instruction::I32Const(message),
                Instruction::Call(err_raise),
                c(0),
                Instruction::Return,
            ]
        };
        let lt_s = pick(Instruction::I32LtS, Instruction::I64LtS);
        let eq = pick(Instruction::I32Eq, Instruction::I64Eq);
        let eqz = pick(Instruction::I32Eqz, Instruction::I64Eqz);
        let and = pick(Instruction::I32And, Instruction::I64And);
        let sub = pick(Instruction::I32Sub, Instruction::I64Sub);
        let mul = pick(Instruction::I32Mul, Instruction::I64Mul);
        let ne = pick(Instruction::I32Ne, Instruction::I64Ne);
        let div = match (wide, unsigned) {
            (false, false) => Instruction::I32DivS,
            (false, true) => Instruction::I32DivU,
            (true, false) => Instruction::I64DivS,
            (true, true) => Instruction::I64DivU,
        };
        // `even exponent ? 1 : -1`, returned.
        let parity_sign = || {
            vec![
                c(1),
                c(-1),
                Instruction::LocalGet(1),
                c(1),
                and.clone(),
                eqz.clone(),
                Instruction::Select,
                Instruction::Return,
                Instruction::End,
            ]
        };
        let mut body = vec![];
        if !unsigned {
            // Negative exponent.
            body.extend([
                Instruction::LocalGet(1),
                c(0),
                lt_s,
                Instruction::If(BlockType::Empty),
                Instruction::LocalGet(0),
                c(1),
                eq.clone(),
                Instruction::If(BlockType::Empty),
                c(1),
                Instruction::Return,
                Instruction::End,
                Instruction::LocalGet(0),
                c(-1),
                eq.clone(),
                Instruction::If(BlockType::Empty),
            ]);
            body.extend(parity_sign());
            body.extend([
                Instruction::LocalGet(0),
                eqz.clone(),
                Instruction::If(BlockType::Empty),
            ]);
            body.extend(raise(division_by_zero));
            body.extend([
                Instruction::End,
                c(0),
                Instruction::Return,
                Instruction::End,
            ]);
        }
        // 0 ^ n is 1 for n == 0 and 0 otherwise; 1 ^ n is 1.
        body.extend([
            Instruction::LocalGet(0),
            eqz.clone(),
            Instruction::If(BlockType::Empty),
            c(1),
            c(0),
            Instruction::LocalGet(1),
            eqz.clone(),
            Instruction::Select,
            Instruction::Return,
            Instruction::End,
            Instruction::LocalGet(0),
            c(1),
            eq.clone(),
            Instruction::If(BlockType::Empty),
            c(1),
            Instruction::Return,
            Instruction::End,
        ]);
        if !unsigned {
            // (-1) ^ n is 1 for even n and -1 for odd n.
            body.extend([
                Instruction::LocalGet(0),
                c(-1),
                eq,
                Instruction::If(BlockType::Empty),
            ]);
            body.extend(parity_sign());
        }
        // result = 1; one checked multiplication per exponent step.
        body.extend([
            c(1),
            Instruction::LocalSet(2),
            Instruction::Block(BlockType::Empty),
            Instruction::Loop(BlockType::Empty),
            Instruction::LocalGet(1),
            eqz,
            Instruction::BrIf(1),
        ]);
        if wide {
            // product = result * base; overflow when product / result != base.
            body.extend([
                Instruction::LocalGet(2),
                Instruction::LocalGet(0),
                mul,
                Instruction::LocalTee(3),
                Instruction::LocalGet(2),
                div,
                Instruction::LocalGet(0),
                ne,
                Instruction::If(BlockType::Empty),
            ]);
            body.extend(raise(overflow));
            body.extend([
                Instruction::End,
                Instruction::LocalGet(3),
                Instruction::LocalSet(2),
            ]);
        } else {
            let extend = if unsigned {
                Instruction::I64ExtendI32U
            } else {
                Instruction::I64ExtendI32S
            };
            body.extend([
                Instruction::LocalGet(2),
                extend.clone(),
                Instruction::LocalGet(0),
                extend,
                Instruction::I64Mul,
                Instruction::LocalTee(3),
            ]);
            if unsigned {
                body.extend([
                    Instruction::I64Const(i64::from(u32::MAX)),
                    Instruction::I64GtU,
                ]);
            } else {
                body.extend([
                    Instruction::LocalGet(3),
                    Instruction::I32WrapI64,
                    Instruction::I64ExtendI32S,
                    Instruction::I64Ne,
                ]);
            }
            body.push(Instruction::If(BlockType::Empty));
            body.extend(raise(overflow));
            body.extend([
                Instruction::End,
                Instruction::LocalGet(3),
                Instruction::I32WrapI64,
                Instruction::LocalSet(2),
            ]);
        }
        body.extend([
            Instruction::LocalGet(1),
            c(1),
            sub,
            Instruction::LocalSet(1),
            Instruction::Br(0),
            Instruction::End,
            Instruction::End,
            Instruction::LocalGet(2),
        ]);
        body
    }
}
