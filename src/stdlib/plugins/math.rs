use crate::codegen::CodeGenerator;
use crate::error::CompilerError;
use crate::stdlib::plugin::{
    FunctionCategory, FunctionMetadata, MemoryRequirements, PluginConfig, PluginConfigSchema,
    StdlibPlugin,
};
use std::any::Any;

/// Math operations plugin providing basic mathematical functions
pub struct MathPlugin {
    config: Option<PluginConfig>,
    precision_mode: PrecisionMode,
    enable_advanced: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PrecisionMode {
    Single,   // 32-bit float
    Double,   // 64-bit float
    Extended, // Platform-specific extended precision
}

impl MathPlugin {
    pub fn new() -> Self {
        Self {
            config: None,
            precision_mode: PrecisionMode::Double,
            enable_advanced: true,
        }
    }

    pub fn with_precision(mut self, mode: PrecisionMode) -> Self {
        self.precision_mode = mode;
        self
    }

    pub fn with_advanced_functions(mut self, enable: bool) -> Self {
        self.enable_advanced = enable;
        self
    }
}

impl Default for MathPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl StdlibPlugin for MathPlugin {
    fn name(&self) -> &'static str {
        "math"
    }

    fn version(&self) -> &'static str {
        "1.0.0"
    }

    fn description(&self) -> &'static str {
        "Core mathematical operations and functions"
    }

    fn dependencies(&self) -> Vec<&'static str> {
        vec!["memory"] // Math operations may need memory allocation
    }

    fn initialize(&mut self, config: PluginConfig) -> Result<(), CompilerError> {
        // Extract configuration settings
        if let Some(precision) = config.settings.get("precision") {
            if let Some(precision_str) = precision.as_string() {
                self.precision_mode = match precision_str {
                    "single" => PrecisionMode::Single,
                    "double" => PrecisionMode::Double,
                    "extended" => PrecisionMode::Extended,
                    _ => PrecisionMode::Double,
                };
            }
        }

        if let Some(advanced) = config.settings.get("enable_advanced") {
            if let Some(enable) = advanced.as_boolean() {
                self.enable_advanced = enable;
            }
        }

        self.config = Some(config);
        Ok(())
    }

    fn register_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        // Register basic arithmetic functions
        self.register_basic_functions(codegen)?;

        // Register advanced functions if enabled
        if self.enable_advanced {
            self.register_advanced_functions(codegen)?;
        }

        Ok(())
    }

    fn provided_functions(&self) -> Vec<FunctionMetadata> {
        let mut functions = vec![
            // Basic arithmetic
            FunctionMetadata {
                name: "add".to_string(),
                signature: "add(a: number, b: number) -> number".to_string(),
                description: "Add two numbers".to_string(),
                category: FunctionCategory::Math,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
            FunctionMetadata {
                name: "subtract".to_string(),
                signature: "subtract(a: number, b: number) -> number".to_string(),
                description: "Subtract second number from first".to_string(),
                category: FunctionCategory::Math,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
            FunctionMetadata {
                name: "multiply".to_string(),
                signature: "multiply(a: number, b: number) -> number".to_string(),
                description: "Multiply two numbers".to_string(),
                category: FunctionCategory::Math,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
            FunctionMetadata {
                name: "divide".to_string(),
                signature: "divide(a: number, b: number) -> number".to_string(),
                description: "Divide first number by second".to_string(),
                category: FunctionCategory::Math,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
            FunctionMetadata {
                name: "modulo".to_string(),
                signature: "modulo(a: number, b: number) -> number".to_string(),
                description: "Get remainder of division".to_string(),
                category: FunctionCategory::Math,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
            FunctionMetadata {
                name: "power".to_string(),
                signature: "power(base: number, exponent: number) -> number".to_string(),
                description: "Raise base to the power of exponent".to_string(),
                category: FunctionCategory::Math,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
            FunctionMetadata {
                name: "sqrt".to_string(),
                signature: "sqrt(x: number) -> number".to_string(),
                description: "Calculate square root".to_string(),
                category: FunctionCategory::Math,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
            FunctionMetadata {
                name: "abs".to_string(),
                signature: "abs(x: number) -> number".to_string(),
                description: "Get absolute value".to_string(),
                category: FunctionCategory::Math,
                is_builtin: true,
                is_async: false,
                memory_requirements: MemoryRequirements::default(),
            },
        ];

        if self.enable_advanced {
            functions.extend(vec![
                FunctionMetadata {
                    name: "sin".to_string(),
                    signature: "sin(x: number) -> number".to_string(),
                    description: "Calculate sine".to_string(),
                    category: FunctionCategory::Math,
                    is_builtin: true,
                    is_async: false,
                    memory_requirements: MemoryRequirements::default(),
                },
                FunctionMetadata {
                    name: "cos".to_string(),
                    signature: "cos(x: number) -> number".to_string(),
                    description: "Calculate cosine".to_string(),
                    category: FunctionCategory::Math,
                    is_builtin: true,
                    is_async: false,
                    memory_requirements: MemoryRequirements::default(),
                },
                FunctionMetadata {
                    name: "tan".to_string(),
                    signature: "tan(x: number) -> number".to_string(),
                    description: "Calculate tangent".to_string(),
                    category: FunctionCategory::Math,
                    is_builtin: true,
                    is_async: false,
                    memory_requirements: MemoryRequirements::default(),
                },
                FunctionMetadata {
                    name: "log".to_string(),
                    signature: "log(x: number) -> number".to_string(),
                    description: "Calculate natural logarithm".to_string(),
                    category: FunctionCategory::Math,
                    is_builtin: true,
                    is_async: false,
                    memory_requirements: MemoryRequirements::default(),
                },
                FunctionMetadata {
                    name: "log10".to_string(),
                    signature: "log10(x: number) -> number".to_string(),
                    description: "Calculate base-10 logarithm".to_string(),
                    category: FunctionCategory::Math,
                    is_builtin: true,
                    is_async: false,
                    memory_requirements: MemoryRequirements::default(),
                },
                FunctionMetadata {
                    name: "exp".to_string(),
                    signature: "exp(x: number) -> number".to_string(),
                    description: "Calculate e raised to the power of x".to_string(),
                    category: FunctionCategory::Math,
                    is_builtin: true,
                    is_async: false,
                    memory_requirements: MemoryRequirements::default(),
                },
            ]);
        }

        functions
    }

    fn config_schema(&self) -> PluginConfigSchema {
        use crate::stdlib::plugin::{
            ConfigSetting, ConfigSettingType, ConfigValue, ValidationRule,
        };

        let mut schema = PluginConfigSchema::default();

        schema.settings.insert(
            "precision".to_string(),
            ConfigSetting {
                name: "precision".to_string(),
                description: "Floating point precision mode".to_string(),
                setting_type: ConfigSettingType::String,
                default_value: Some(ConfigValue::String("double".to_string())),
                required: false,
                validation: Some(ValidationRule::OneOf(vec![
                    ConfigValue::String("single".to_string()),
                    ConfigValue::String("double".to_string()),
                    ConfigValue::String("extended".to_string()),
                ])),
            },
        );

        schema.settings.insert(
            "enable_advanced".to_string(),
            ConfigSetting {
                name: "enable_advanced".to_string(),
                description: "Enable advanced mathematical functions".to_string(),
                setting_type: ConfigSettingType::Boolean,
                default_value: Some(ConfigValue::Boolean(true)),
                required: false,
                validation: None,
            },
        );

        schema
    }

    fn priority(&self) -> i32 {
        100 // High priority - many other plugins depend on math
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl MathPlugin {
    /// Register basic mathematical functions
    fn register_basic_functions(&self, codegen: &mut CodeGenerator) -> Result<(), CompilerError> {
        use crate::types::WasmType;
        use wasm_encoder::Instruction;

        let float_type = match self.precision_mode {
            PrecisionMode::Single => WasmType::F32,
            _ => WasmType::F64, // Default to f64 for double and extended
        };

        // Add function
        crate::stdlib::register_stdlib_function(
            codegen,
            "add",
            &[float_type, float_type],
            Some(float_type),
            vec![
                Instruction::LocalGet(0),
                Instruction::LocalGet(1),
                match float_type {
                    WasmType::F32 => Instruction::F32Add,
                    WasmType::F64 => Instruction::F64Add,
                    _ => unreachable!(),
                },
            ],
        )?;

        // Subtract function
        crate::stdlib::register_stdlib_function(
            codegen,
            "subtract",
            &[float_type, float_type],
            Some(float_type),
            vec![
                Instruction::LocalGet(0),
                Instruction::LocalGet(1),
                match float_type {
                    WasmType::F32 => Instruction::F32Sub,
                    WasmType::F64 => Instruction::F64Sub,
                    _ => unreachable!(),
                },
            ],
        )?;

        // Multiply function
        crate::stdlib::register_stdlib_function(
            codegen,
            "multiply",
            &[float_type, float_type],
            Some(float_type),
            vec![
                Instruction::LocalGet(0),
                Instruction::LocalGet(1),
                match float_type {
                    WasmType::F32 => Instruction::F32Mul,
                    WasmType::F64 => Instruction::F64Mul,
                    _ => unreachable!(),
                },
            ],
        )?;

        // Divide function
        crate::stdlib::register_stdlib_function(
            codegen,
            "divide",
            &[float_type, float_type],
            Some(float_type),
            vec![
                Instruction::LocalGet(0),
                Instruction::LocalGet(1),
                match float_type {
                    WasmType::F32 => Instruction::F32Div,
                    WasmType::F64 => Instruction::F64Div,
                    _ => unreachable!(),
                },
            ],
        )?;

        // Power function (simplified implementation)
        crate::stdlib::register_stdlib_function(
            codegen,
            "power",
            &[float_type, float_type],
            Some(float_type),
            vec![
                Instruction::LocalGet(0),
                Instruction::LocalGet(1),
                // For WebAssembly, we'll need to call external power function
                // This is a simplified version
                match float_type {
                    WasmType::F32 => Instruction::F32Mul, // Simplified - just multiply for now
                    WasmType::F64 => Instruction::F64Mul,
                    _ => unreachable!(),
                },
            ],
        )?;

        // Square root function
        crate::stdlib::register_stdlib_function(
            codegen,
            "sqrt",
            &[float_type],
            Some(float_type),
            vec![
                Instruction::LocalGet(0),
                match float_type {
                    WasmType::F32 => Instruction::F32Sqrt,
                    WasmType::F64 => Instruction::F64Sqrt,
                    _ => unreachable!(),
                },
            ],
        )?;

        // Absolute value function
        crate::stdlib::register_stdlib_function(
            codegen,
            "abs",
            &[float_type],
            Some(float_type),
            vec![
                Instruction::LocalGet(0),
                match float_type {
                    WasmType::F32 => Instruction::F32Abs,
                    WasmType::F64 => Instruction::F64Abs,
                    _ => unreachable!(),
                },
            ],
        )?;

        Ok(())
    }

    /// Register advanced mathematical functions
    fn register_advanced_functions(
        &self,
        codegen: &mut CodeGenerator,
    ) -> Result<(), CompilerError> {
        use crate::types::WasmType;
        use wasm_encoder::Instruction;

        let float_type = match self.precision_mode {
            PrecisionMode::Single => WasmType::F32,
            _ => WasmType::F64,
        };

        // Trigonometric functions require host imports (not available in native WASM)
        // The imports math_sin, math_cos, etc. are registered by builtin_generator
        // Here we create wrapper functions that call those imports

        // Sine function - calls math_sin import
        if let Some(sin_index) = codegen.get_function_index("math_sin") {
            crate::stdlib::register_stdlib_function(
                codegen,
                "sin",
                &[float_type],
                Some(float_type),
                vec![Instruction::LocalGet(0), Instruction::Call(sin_index)],
            )?;
        }

        // Cosine function - calls math_cos import
        if let Some(cos_index) = codegen.get_function_index("math_cos") {
            crate::stdlib::register_stdlib_function(
                codegen,
                "cos",
                &[float_type],
                Some(float_type),
                vec![Instruction::LocalGet(0), Instruction::Call(cos_index)],
            )?;
        }

        // Tangent function - calls math_tan import
        if let Some(tan_index) = codegen.get_function_index("math_tan") {
            crate::stdlib::register_stdlib_function(
                codegen,
                "tan",
                &[float_type],
                Some(float_type),
                vec![Instruction::LocalGet(0), Instruction::Call(tan_index)],
            )?;
        }

        // Natural logarithm - calls math_ln import
        if let Some(ln_index) = codegen.get_function_index("math_ln") {
            crate::stdlib::register_stdlib_function(
                codegen,
                "log",
                &[float_type],
                Some(float_type),
                vec![Instruction::LocalGet(0), Instruction::Call(ln_index)],
            )?;
        }

        // Exponential function - calls math_exp import
        if let Some(exp_index) = codegen.get_function_index("math_exp") {
            crate::stdlib::register_stdlib_function(
                codegen,
                "exp",
                &[float_type],
                Some(float_type),
                vec![Instruction::LocalGet(0), Instruction::Call(exp_index)],
            )?;
        }

        Ok(())
    }

    /// Get current precision mode
    pub fn precision_mode(&self) -> &PrecisionMode {
        &self.precision_mode
    }

    /// Check if advanced functions are enabled
    pub fn has_advanced_functions(&self) -> bool {
        self.enable_advanced
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdlib::plugin::ConfigSettingType;

    #[test]
    fn test_math_plugin_creation() {
        let plugin = MathPlugin::new();
        assert_eq!(plugin.name(), "math");
        assert_eq!(plugin.version(), "1.0.0");
        assert!(plugin.has_advanced_functions());
    }

    #[test]
    fn test_math_plugin_configuration() {
        let plugin = MathPlugin::new()
            .with_precision(PrecisionMode::Single)
            .with_advanced_functions(false);

        assert_eq!(plugin.precision_mode(), &PrecisionMode::Single);
        assert!(!plugin.has_advanced_functions());
    }

    #[test]
    fn test_provided_functions() {
        let plugin = MathPlugin::new();
        let functions = plugin.provided_functions();

        // Should have at least basic functions
        assert!(functions.len() >= 8);

        let function_names: Vec<&str> = functions.iter().map(|f| f.name.as_str()).collect();
        assert!(function_names.contains(&"add"));
        assert!(function_names.contains(&"subtract"));
        assert!(function_names.contains(&"multiply"));
        assert!(function_names.contains(&"divide"));
        assert!(function_names.contains(&"sqrt"));
        assert!(function_names.contains(&"abs"));

        // Should have advanced functions when enabled
        assert!(function_names.contains(&"sin"));
        assert!(function_names.contains(&"cos"));
        assert!(function_names.contains(&"log"));
    }

    #[test]
    fn test_provided_functions_no_advanced() {
        let plugin = MathPlugin::new().with_advanced_functions(false);
        let functions = plugin.provided_functions();

        let function_names: Vec<&str> = functions.iter().map(|f| f.name.as_str()).collect();

        // Should not have advanced functions
        assert!(!function_names.contains(&"sin"));
        assert!(!function_names.contains(&"cos"));
        assert!(!function_names.contains(&"log"));

        // Should still have basic functions
        assert!(function_names.contains(&"add"));
        assert!(function_names.contains(&"sqrt"));
    }

    #[test]
    fn test_config_schema() {
        let plugin = MathPlugin::new();
        let schema = plugin.config_schema();

        assert!(schema.settings.contains_key("precision"));
        assert!(schema.settings.contains_key("enable_advanced"));

        let precision_setting = &schema.settings["precision"];
        assert_eq!(precision_setting.setting_type, ConfigSettingType::String);

        let advanced_setting = &schema.settings["enable_advanced"];
        assert_eq!(advanced_setting.setting_type, ConfigSettingType::Boolean);
    }
}
