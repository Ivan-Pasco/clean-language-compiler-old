use crate::runtime::runtime_trait::{OptimizationLevel, RuntimeConfig};
use crate::targets::target_config::{Target, TargetType};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Target-specific optimization pipeline
pub struct TargetOptimizer;

/// Optimization profile for different scenarios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizationProfile {
    /// Profile name
    pub name: String,
    /// Description of the optimization goals
    pub description: String,
    /// Optimization level to use
    pub optimization_level: OptimizationLevel,
    /// Target-specific settings
    pub target_settings: HashMap<String, String>,
    /// Compiler flags to apply
    pub compiler_flags: Vec<String>,
    /// Whether to enable debug information
    pub debug_info: bool,
}

impl TargetOptimizer {
    /// Apply target-specific optimizations to runtime configuration
    pub fn optimize_for_target(config: &mut RuntimeConfig, target: &Target) {
        // Set optimization level based on target requirements
        config.optimization_level = match target.target_type {
            TargetType::Web => {
                // Web applications need small size for fast downloads
                if target.optimizations.optimize_for_size {
                    OptimizationLevel::SpeedAndSize
                } else {
                    OptimizationLevel::Speed
                }
            }
            TargetType::NodeJS => {
                // Node.js applications can afford larger size for better performance
                OptimizationLevel::Speed
            }
            TargetType::Native => {
                // Native applications want maximum performance
                OptimizationLevel::SpeedAndSize
            }
            TargetType::Embedded => {
                // Embedded systems need minimal size
                OptimizationLevel::SpeedAndSize
            }
            TargetType::WASI => {
                // WASI applications balance portability and performance
                OptimizationLevel::Speed
            }
        };

        // Configure memory based on target constraints
        if let Some(max_memory) = target.capabilities.max_memory_size {
            config.memory_config.static_memory_maximum =
                max_memory.min(config.memory_config.static_memory_maximum);
        }

        // Adjust feature flags based on target capabilities
        config.async_support = config.async_support && target.capabilities.async_support;
        config.threads_support = config.threads_support && target.capabilities.threads_support;
        config.simd_support = config.simd_support && target.capabilities.simd_support;
        config.bulk_memory = config.bulk_memory && target.capabilities.bulk_memory;
        config.reference_types = config.reference_types && target.capabilities.reference_types;

        // Apply target-specific settings
        for (key, value) in &target.environment {
            config.target_settings.insert(key.clone(), value.clone());
        }

        // Add target-specific compiler flags
        for flag in &target.optimizations.custom_flags {
            config.target_settings.insert(
                format!("flag_{}", config.target_settings.len()),
                flag.clone(),
            );
        }
    }

    /// Get predefined optimization profiles
    pub fn get_optimization_profiles() -> Vec<OptimizationProfile> {
        vec![
            OptimizationProfile::development(),
            OptimizationProfile::production(),
            OptimizationProfile::size_optimized(),
            OptimizationProfile::speed_optimized(),
            OptimizationProfile::debug(),
        ]
    }

    /// Get recommended optimization profile for a target
    pub fn get_recommended_profile(target: &Target) -> OptimizationProfile {
        match target.target_type {
            TargetType::Web => OptimizationProfile::size_optimized(),
            TargetType::NodeJS => OptimizationProfile::speed_optimized(),
            TargetType::Native => OptimizationProfile::production(),
            TargetType::Embedded => OptimizationProfile::size_optimized(),
            TargetType::WASI => OptimizationProfile::production(),
        }
    }

    /// Apply optimization profile to runtime configuration
    pub fn apply_optimization_profile(config: &mut RuntimeConfig, profile: &OptimizationProfile) {
        config.optimization_level = profile.optimization_level;
        config.debug_info = profile.debug_info;

        // Merge target settings
        for (key, value) in &profile.target_settings {
            config.target_settings.insert(key.clone(), value.clone());
        }

        // Add compiler flags as target settings
        for (index, flag) in profile.compiler_flags.iter().enumerate() {
            config
                .target_settings
                .insert(format!("compiler_flag_{index}"), flag.clone());
        }
    }

    /// Validate optimization settings against target capabilities
    pub fn validate_optimization_settings(target: &Target, config: &RuntimeConfig) -> Vec<String> {
        let mut warnings = Vec::new();

        // Check if optimization level is appropriate for target
        match (target.target_type, config.optimization_level) {
            (TargetType::Embedded, OptimizationLevel::None) => {
                warnings.push(
                    "Consider enabling optimization for embedded targets to reduce code size"
                        .to_string(),
                );
            }
            (TargetType::Web, OptimizationLevel::None) => {
                warnings.push(
                    "Web targets should use optimization to reduce download time".to_string(),
                );
            }
            _ => {}
        }

        // Check memory settings
        if let Some(max_memory) = target.capabilities.max_memory_size {
            if config.memory_config.static_memory_maximum > max_memory {
                warnings.push(format!(
                    "Memory configuration ({} bytes) exceeds target limit ({} bytes)",
                    config.memory_config.static_memory_maximum, max_memory
                ));
            }
        }

        // Check feature compatibility
        if config.async_support && !target.capabilities.async_support {
            warnings.push(format!(
                "Async support requested but not supported by target '{}'",
                target.name
            ));
        }

        if config.threads_support && !target.capabilities.threads_support {
            warnings.push(format!(
                "Thread support requested but not supported by target '{}'",
                target.name
            ));
        }

        if config.simd_support && !target.capabilities.simd_support {
            warnings.push(format!(
                "SIMD support requested but not supported by target '{}'",
                target.name
            ));
        }

        warnings
    }
}

impl OptimizationProfile {
    /// Development profile - fast compilation, debugging enabled
    pub fn development() -> Self {
        Self {
            name: "development".to_string(),
            description: "Fast compilation with debugging support".to_string(),
            optimization_level: OptimizationLevel::None,
            target_settings: {
                let mut settings = HashMap::new();
                settings.insert("debug_symbols".to_string(), "true".to_string());
                settings.insert("fast_compilation".to_string(), "true".to_string());
                settings
            },
            compiler_flags: vec!["--debug".to_string(), "--no-optimize".to_string()],
            debug_info: true,
        }
    }

    /// Production profile - balanced optimization
    pub fn production() -> Self {
        Self {
            name: "production".to_string(),
            description: "Balanced optimization for production use".to_string(),
            optimization_level: OptimizationLevel::Speed,
            target_settings: {
                let mut settings = HashMap::new();
                settings.insert("production_mode".to_string(), "true".to_string());
                settings.insert("optimize_imports".to_string(), "true".to_string());
                settings
            },
            compiler_flags: vec!["--optimize".to_string(), "--strip-debug".to_string()],
            debug_info: false,
        }
    }

    /// Size-optimized profile - minimize code size
    pub fn size_optimized() -> Self {
        Self {
            name: "size".to_string(),
            description: "Optimize for minimal code size".to_string(),
            optimization_level: OptimizationLevel::SpeedAndSize,
            target_settings: {
                let mut settings = HashMap::new();
                settings.insert("optimize_size".to_string(), "true".to_string());
                settings.insert(
                    "dead_code_elimination".to_string(),
                    "aggressive".to_string(),
                );
                settings
            },
            compiler_flags: vec![
                "--optimize-size".to_string(),
                "--strip-all".to_string(),
                "--eliminate-dead-code".to_string(),
            ],
            debug_info: false,
        }
    }

    /// Speed-optimized profile - maximize execution speed
    pub fn speed_optimized() -> Self {
        Self {
            name: "speed".to_string(),
            description: "Optimize for maximum execution speed".to_string(),
            optimization_level: OptimizationLevel::Speed,
            target_settings: {
                let mut settings = HashMap::new();
                settings.insert("optimize_speed".to_string(), "true".to_string());
                settings.insert("inline_functions".to_string(), "aggressive".to_string());
                settings.insert("vectorize".to_string(), "true".to_string());
                settings
            },
            compiler_flags: vec![
                "--optimize-speed".to_string(),
                "--inline-functions".to_string(),
                "--vectorize".to_string(),
            ],
            debug_info: false,
        }
    }

    /// Debug profile - maximum debugging information
    pub fn debug() -> Self {
        Self {
            name: "debug".to_string(),
            description: "Maximum debugging information and runtime checks".to_string(),
            optimization_level: OptimizationLevel::None,
            target_settings: {
                let mut settings = HashMap::new();
                settings.insert("debug_mode".to_string(), "full".to_string());
                settings.insert("runtime_checks".to_string(), "true".to_string());
                settings.insert("preserve_names".to_string(), "true".to_string());
                settings
            },
            compiler_flags: vec![
                "--debug-full".to_string(),
                "--runtime-checks".to_string(),
                "--preserve-names".to_string(),
            ],
            debug_info: true,
        }
    }
}

impl std::fmt::Display for OptimizationProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} - {}", self.name, self.description)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::targets::Target;

    #[test]
    fn test_optimize_for_target() {
        let mut config = RuntimeConfig::default();
        let web_target = Target::web();

        TargetOptimizer::optimize_for_target(&mut config, &web_target);

        // Web target should optimize for size
        assert!(matches!(
            config.optimization_level,
            OptimizationLevel::SpeedAndSize
        ));
    }

    #[test]
    fn test_get_optimization_profiles() {
        let profiles = TargetOptimizer::get_optimization_profiles();
        assert_eq!(profiles.len(), 5);

        let profile_names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert!(profile_names.contains(&"development"));
        assert!(profile_names.contains(&"production"));
        assert!(profile_names.contains(&"size"));
        assert!(profile_names.contains(&"speed"));
        assert!(profile_names.contains(&"debug"));
    }

    #[test]
    fn test_get_recommended_profile() {
        let web_target = Target::web();
        let profile = TargetOptimizer::get_recommended_profile(&web_target);
        assert_eq!(profile.name, "size"); // Web should recommend size optimization

        let nodejs_target = Target::nodejs();
        let profile = TargetOptimizer::get_recommended_profile(&nodejs_target);
        assert_eq!(profile.name, "speed"); // Node.js should recommend speed optimization
    }

    #[test]
    fn test_apply_optimization_profile() {
        let mut config = RuntimeConfig::default();
        let profile = OptimizationProfile::production();

        TargetOptimizer::apply_optimization_profile(&mut config, &profile);

        assert_eq!(config.optimization_level, OptimizationLevel::Speed);
        assert!(!config.debug_info);
    }

    #[test]
    fn test_validate_optimization_settings() {
        let embedded_target = Target::embedded();
        let mut config = RuntimeConfig::default();
        config.memory_config.static_memory_maximum = 10 * 1024 * 1024; // 10MB - too much for embedded

        let warnings = TargetOptimizer::validate_optimization_settings(&embedded_target, &config);
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|w| w.to_lowercase().contains("memory")));
    }
}
