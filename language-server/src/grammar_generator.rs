/*!
 * TextMate Grammar Generator
 *
 * Generates TextMate grammar patterns from plugin language definitions.
 * This allows plugins to contribute syntax highlighting rules dynamically.
 */

use clean_language_compiler::plugins::LanguageRegistry;
use serde_json::{json, Value};

/// Simple regex escape function for TextMate patterns
fn escape_regex(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        match c {
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$' => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
    }
    result
}

/// Generates TextMate grammar patterns from language registry definitions
pub struct GrammarGenerator;

impl GrammarGenerator {
    /// Generate a complete TextMate grammar JSON from the language registry
    ///
    /// This generates a grammar that can be used to extend the base Clean Language
    /// syntax highlighting with plugin-defined keywords and blocks.
    ///
    /// # Arguments
    /// * `registry` - The language registry containing plugin definitions
    ///
    /// # Returns
    /// A JSON Value representing the TextMate grammar
    pub fn generate(registry: &LanguageRegistry) -> Value {
        let mut patterns = Vec::new();

        // Generate block patterns
        let blocks: Vec<&str> = registry.all_blocks();
        if !blocks.is_empty() {
            patterns.push(Self::generate_block_patterns(&blocks));
        }

        // Generate keyword patterns
        let keywords: Vec<&str> = registry.all_keywords();
        if !keywords.is_empty() {
            patterns.push(Self::generate_keyword_patterns(&keywords));
        }

        // Generate type patterns
        let types: Vec<&str> = registry.all_types();
        if !types.is_empty() {
            patterns.push(Self::generate_type_patterns(&types));
        }

        // Generate function patterns
        let functions: Vec<&str> = registry.all_functions();
        if !functions.is_empty() {
            patterns.push(Self::generate_function_patterns(&functions));
        }

        json!({
            "name": "Clean Language Plugin Extensions",
            "scopeName": "source.clean.plugins",
            "patterns": patterns,
            "repository": {
                "plugin-blocks": Self::generate_block_repository(&blocks),
                "plugin-keywords": Self::generate_keyword_repository(&keywords),
                "plugin-types": Self::generate_type_repository(&types),
                "plugin-functions": Self::generate_function_repository(&functions)
            }
        })
    }

    /// Generate patterns for plugin blocks (e.g., "data:", "endpoints:")
    fn generate_block_patterns(blocks: &[&str]) -> Value {
        if blocks.is_empty() {
            return json!({});
        }

        let block_pattern = blocks.join("|");

        json!({
            "name": "keyword.control.plugin-block.clean",
            "match": format!(r"^\s*({})\s*:", block_pattern),
            "captures": {
                "1": {
                    "name": "keyword.control.plugin-block.clean"
                }
            }
        })
    }

    /// Generate patterns for plugin keywords
    fn generate_keyword_patterns(keywords: &[&str]) -> Value {
        if keywords.is_empty() {
            return json!({});
        }

        let keyword_pattern = keywords
            .iter()
            .map(|k| escape_regex(k))
            .collect::<Vec<_>>()
            .join("|");

        json!({
            "name": "keyword.other.plugin.clean",
            "match": format!(r"\b({})\b", keyword_pattern)
        })
    }

    /// Generate patterns for plugin types
    fn generate_type_patterns(types: &[&str]) -> Value {
        if types.is_empty() {
            return json!({});
        }

        let type_pattern = types
            .iter()
            .map(|t| escape_regex(t))
            .collect::<Vec<_>>()
            .join("|");

        json!({
            "name": "support.type.plugin.clean",
            "match": format!(r"\b({})\b", type_pattern)
        })
    }

    /// Generate patterns for plugin functions
    fn generate_function_patterns(functions: &[&str]) -> Value {
        if functions.is_empty() {
            return json!({});
        }

        let func_pattern = functions
            .iter()
            .map(|f| escape_regex(f))
            .collect::<Vec<_>>()
            .join("|");

        json!({
            "name": "support.function.plugin.clean",
            "match": format!(r"\b({})\s*\(", func_pattern),
            "captures": {
                "1": {
                    "name": "support.function.plugin.clean"
                }
            }
        })
    }

    /// Generate repository entry for block patterns
    fn generate_block_repository(blocks: &[&str]) -> Value {
        let mut patterns = Vec::new();

        for block in blocks {
            patterns.push(json!({
                "name": format!("keyword.control.{}.clean", block),
                "match": format!(r"^\s*({})\s*:", escape_regex(block)),
                "captures": {
                    "1": {
                        "name": format!("keyword.control.{}.clean", block)
                    }
                }
            }));
        }

        json!({
            "patterns": patterns
        })
    }

    /// Generate repository entry for keyword patterns
    fn generate_keyword_repository(keywords: &[&str]) -> Value {
        let mut patterns = Vec::new();

        for keyword in keywords {
            patterns.push(json!({
                "name": format!("keyword.other.{}.clean", keyword),
                "match": format!(r"\b{}\b", escape_regex(keyword))
            }));
        }

        json!({
            "patterns": patterns
        })
    }

    /// Generate repository entry for type patterns
    fn generate_type_repository(types: &[&str]) -> Value {
        let mut patterns = Vec::new();

        for type_name in types {
            patterns.push(json!({
                "name": format!("support.type.{}.clean", type_name),
                "match": format!(r"\b{}\b", escape_regex(type_name))
            }));
        }

        json!({
            "patterns": patterns
        })
    }

    /// Generate repository entry for function patterns
    fn generate_function_repository(functions: &[&str]) -> Value {
        let mut patterns = Vec::new();

        for func in functions {
            patterns.push(json!({
                "name": format!("support.function.{}.clean", func),
                "match": format!(r"\b{}\s*\(", escape_regex(func)),
                "captures": {
                    "1": {
                        "name": format!("support.function.{}.clean", func)
                    }
                }
            }));
        }

        json!({
            "patterns": patterns
        })
    }

    /// Generate semantic token types for plugin elements
    ///
    /// Returns a list of (token_type, token_modifiers) for use with
    /// LSP semantic tokens.
    pub fn generate_semantic_token_types() -> Vec<(&'static str, Vec<&'static str>)> {
        vec![
            ("keyword", vec!["plugin"]),         // Plugin keywords
            ("type", vec!["plugin"]),            // Plugin types
            ("function", vec!["plugin"]),        // Plugin functions
            ("namespace", vec!["plugin"]),       // Plugin blocks
        ]
    }

    /// Check if a word is a plugin-defined element and return its token type
    ///
    /// # Arguments
    /// * `word` - The word to check
    /// * `registry` - The language registry
    ///
    /// # Returns
    /// A tuple of (token_type_index, token_modifiers_bitset) if the word
    /// is a plugin element, None otherwise.
    pub fn get_semantic_token_type(word: &str, registry: &LanguageRegistry) -> Option<(u32, u32)> {
        // Check blocks (namespace)
        if registry.has_block(word) {
            return Some((3, 1)); // namespace with plugin modifier
        }

        // Check keywords
        if registry.get_keyword(word).is_some() {
            return Some((0, 1)); // keyword with plugin modifier
        }

        // Check types
        if registry.get_type(word).is_some() {
            return Some((1, 1)); // type with plugin modifier
        }

        // Check functions
        if registry.get_function(word).is_some() {
            return Some((2, 1)); // function with plugin modifier
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clean_language_compiler::plugins::{
        PluginCompletionDef, PluginFunctionDef, PluginKeyword, PluginLanguage, PluginManifest,
        PluginTypeDef,
    };
    use std::collections::HashMap;

    fn create_test_manifest() -> PluginManifest {
        use clean_language_compiler::plugins::plugin_abi::{
            PluginCompatibility, PluginExports, PluginHandles, PluginInfo,
        };

        PluginManifest {
            plugin: PluginInfo {
                name: "test.plugin".to_string(),
                version: "1.0.0".to_string(),
                description: "Test plugin".to_string(),
                author: "Test".to_string(),
            },
            compatibility: PluginCompatibility::default(),
            handles: clean_language_compiler::plugins::plugin_abi::PluginHandles {
                blocks: vec!["testblock".to_string()],
            },
            exports: PluginExports::default(),
            bridge: Default::default(),
            language: PluginLanguage {
                blocks: vec!["data".to_string(), "endpoints".to_string()],
                keywords: vec![
                    PluginKeyword {
                        name: "find".to_string(),
                        description: "Find records".to_string(),
                        context: "expression".to_string(),
                    },
                    PluginKeyword {
                        name: "where".to_string(),
                        description: "Filter".to_string(),
                        context: "block".to_string(),
                    },
                ],
                types: vec![PluginTypeDef {
                    name: "Model".to_string(),
                    description: "Data model".to_string(),
                }],
                functions: vec![PluginFunctionDef {
                    name: "Data.tx".to_string(),
                    signature: "Data.tx: block".to_string(),
                    description: "Transaction".to_string(),
                }],
                completions: vec![],
                owns_paths: vec![],
            },
        }
    }

    #[test]
    fn test_generate_grammar() {
        let manifest = create_test_manifest();
        let mut manifests = HashMap::new();
        manifests.insert("test.plugin".to_string(), manifest);

        let registry = LanguageRegistry::from_manifests(&manifests);
        let grammar = GrammarGenerator::generate(&registry);

        assert!(grammar["name"].is_string());
        assert!(grammar["scopeName"].is_string());
        assert!(grammar["patterns"].is_array());
    }

    #[test]
    fn test_generate_block_patterns() {
        let blocks = vec!["data", "endpoints"];
        let pattern = GrammarGenerator::generate_block_patterns(&blocks);

        assert!(pattern["match"].as_str().unwrap().contains("data"));
        assert!(pattern["match"].as_str().unwrap().contains("endpoints"));
    }

    #[test]
    fn test_get_semantic_token_type() {
        let manifest = create_test_manifest();
        let mut manifests = HashMap::new();
        manifests.insert("test.plugin".to_string(), manifest);

        let registry = LanguageRegistry::from_manifests(&manifests);

        // Block
        let block_token = GrammarGenerator::get_semantic_token_type("data", &registry);
        assert!(block_token.is_some());

        // Keyword
        let keyword_token = GrammarGenerator::get_semantic_token_type("find", &registry);
        assert!(keyword_token.is_some());

        // Type
        let type_token = GrammarGenerator::get_semantic_token_type("Model", &registry);
        assert!(type_token.is_some());

        // Function
        let func_token = GrammarGenerator::get_semantic_token_type("Data.tx", &registry);
        assert!(func_token.is_some());

        // Unknown
        let unknown_token = GrammarGenerator::get_semantic_token_type("unknown", &registry);
        assert!(unknown_token.is_none());
    }
}
