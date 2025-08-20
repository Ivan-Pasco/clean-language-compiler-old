pub mod target_config;
pub mod optimization;

use crate::error::CompilerError;
use crate::runtime::runtime_trait::{RuntimeType, RuntimeConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

pub use target_config::*;
pub use optimization::*;

/// Target manager for handling different compilation targets
pub struct TargetManager;

impl TargetManager {
    /// Get all available compilation targets
    pub fn get_available_targets() -> Vec<Target> {
        vec![
            Target::web(),
            Target::nodejs(),
            Target::native(),
            Target::embedded(),
            Target::wasi(),
        ]
    }
    
    /// Get a target by name
    pub fn get_target(name: &str) -> Result<Target, CompilerError> {
        let targets = Self::get_available_targets();
        targets.into_iter()
            .find(|t| t.name == name)
            .ok_or_else(|| CompilerError::runtime_error(
                format!("Unknown target: {name}"),
                None,
                None,
            ))
    }
    
    /// Auto-detect the best target for the current environment
    pub fn auto_detect_target() -> Target {
        // Simple heuristic - could be enhanced with more detection logic
        if cfg!(target_arch = "wasm32") {
            Target::web()
        } else if std::env::var("NODE_ENV").is_ok() {
            Target::nodejs()
        } else {
            Target::native()
        }
    }
    
    /// Validate that a target is compatible with the runtime configuration
    pub fn validate_target_runtime_compatibility(
        target: &Target, 
        runtime_config: &RuntimeConfig
    ) -> Result<(), CompilerError> {
        // Check if the target supports the requested runtime features
        if runtime_config.async_support && !target.capabilities.async_support {
            return Err(CompilerError::runtime_error(
                format!("Target '{}' does not support async operations", target.name),
                None,
                None,
            ));
        }
        
        if runtime_config.threads_support && !target.capabilities.threads_support {
            return Err(CompilerError::runtime_error(
                format!("Target '{}' does not support threading", target.name),
                None,
                None,
            ));
        }
        
        if runtime_config.simd_support && !target.capabilities.simd_support {
            return Err(CompilerError::runtime_error(
                format!("Target '{}' does not support SIMD operations", target.name),
                None,
                None,
            ));
        }
        
        Ok(())
    }
    
    /// Get recommended runtime configuration for a target
    pub fn get_recommended_runtime_config(target: &Target) -> RuntimeConfig {
        let mut config = RuntimeConfig::default();
        
        // Set runtime type based on target preference
        config.runtime_type = target.runtime_preference;
        
        // Configure features based on target capabilities
        config.async_support = target.capabilities.async_support;
        config.threads_support = target.capabilities.threads_support;
        config.simd_support = target.capabilities.simd_support;
        config.bulk_memory = target.capabilities.bulk_memory;
        config.reference_types = target.capabilities.reference_types;
        
        // Set optimization level based on target
        config.optimization_level = match target.target_type {
            TargetType::Web => crate::runtime::runtime_trait::OptimizationLevel::SpeedAndSize,
            TargetType::Embedded => crate::runtime::runtime_trait::OptimizationLevel::SpeedAndSize,
            _ => crate::runtime::runtime_trait::OptimizationLevel::Speed,
        };
        
        // Configure memory settings based on target constraints
        if target.target_type == TargetType::Embedded {
            config.memory_config.static_memory_maximum = 1024 * 1024; // 1MB for embedded
        }
        
        config
    }
    
    /// Get optimization recommendations for a target
    pub fn get_optimization_recommendations(target: &Target) -> Vec<String> {
        let mut recommendations = Vec::new();
        
        match target.target_type {
            TargetType::Web => {
                recommendations.push("Enable size optimization for faster downloads".to_string());
                recommendations.push("Use SIMD for performance-critical computations".to_string());
                recommendations.push("Enable bulk memory for efficient data processing".to_string());
            }
            TargetType::NodeJS => {
                recommendations.push("Enable async support for Node.js integration".to_string());
                recommendations.push("Use threading for CPU-intensive tasks".to_string());
                recommendations.push("Optimize for speed over size".to_string());
            }
            TargetType::Native => {
                recommendations.push("Enable all features for maximum performance".to_string());
                recommendations.push("Use highest optimization level".to_string());
                recommendations.push("Enable debugging symbols in development".to_string());
            }
            TargetType::Embedded => {
                recommendations.push("Minimize memory usage".to_string());
                recommendations.push("Disable unused features to reduce code size".to_string());
                recommendations.push("Use size optimization over speed".to_string());
            }
            TargetType::WASI => {
                recommendations.push("Enable WASI imports for system integration".to_string());
                recommendations.push("Use portable optimization settings".to_string());
                recommendations.push("Enable bulk memory for data processing".to_string());
            }
        }
        
        recommendations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_available_targets() {
        let targets = TargetManager::get_available_targets();
        assert!(!targets.is_empty(), "Should have available targets");
        
        // Check that all standard targets are present
        let target_names: Vec<&str> = targets.iter().map(|t| t.name.as_str()).collect();
        assert!(target_names.contains(&"web"));
        assert!(target_names.contains(&"nodejs"));
        assert!(target_names.contains(&"native"));
        assert!(target_names.contains(&"embedded"));
        assert!(target_names.contains(&"wasi"));
    }

    #[test]
    fn test_get_target_by_name() {
        let web_target = TargetManager::get_target("web");
        assert!(web_target.is_ok());
        assert_eq!(web_target.unwrap().name, "web");
        
        let invalid_target = TargetManager::get_target("invalid");
        assert!(invalid_target.is_err());
    }

    #[test]
    fn test_auto_detect_target() {
        let target = TargetManager::auto_detect_target();
        assert!(!target.name.is_empty());
    }

    #[test]
    fn test_target_runtime_compatibility() {
        let web_target = Target::web();
        let runtime_config = RuntimeConfig::default();
        
        let result = TargetManager::validate_target_runtime_compatibility(&web_target, &runtime_config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_recommended_runtime_config() {
        let web_target = Target::web();
        let config = TargetManager::get_recommended_runtime_config(&web_target);
        
        assert_eq!(config.runtime_type, web_target.runtime_preference);
    }

    #[test]
    fn test_get_optimization_recommendations() {
        let web_target = Target::web();
        let recommendations = TargetManager::get_optimization_recommendations(&web_target);
        
        assert!(!recommendations.is_empty());
        assert!(recommendations.iter().any(|r| r.contains("size optimization")));
    }
}