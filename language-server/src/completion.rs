/*
 * Clean Language Server - Completion Provider
 *
 * Provides intelligent autocompletion for Clean Language including:
 * - Keywords and syntax patterns
 * - Built-in functions and methods
 * - Type annotations
 * - Apply-block completions
 * - Language constructs
 * - Plugin-provided completions (dynamic)
 */

use clean_language_compiler::plugins::{
    LanguageRegistry, PluginCompletionItem, PluginCompletionKind, PluginLspContext, PluginRegistry,
};
use ropey::Rope;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

pub struct CompletionProvider {
    /// Plugin registry for dynamic completions (WASM plugins)
    plugin_registry: Option<Arc<PluginRegistry>>,
    /// Language registry for static completions (plugin.toml definitions)
    language_registry: Option<Arc<LanguageRegistry>>,
}

impl CompletionProvider {
    pub fn new() -> Self {
        Self {
            plugin_registry: None,
            language_registry: None,
        }
    }

    /// Create a completion provider with both plugin and language registries
    pub fn with_language_registry(
        plugin_registry: Arc<PluginRegistry>,
        language_registry: Arc<LanguageRegistry>,
    ) -> Self {
        Self {
            plugin_registry: Some(plugin_registry),
            language_registry: Some(language_registry),
        }
    }

    pub async fn provide_completions(
        &self,
        text: &Rope,
        position: Position,
    ) -> Vec<CompletionItem> {
        let mut completions = Vec::new();

        // Get the current line and character context
        let line_idx = position.line as usize;
        let char_idx = position.character as usize;

        if line_idx >= text.len_lines() {
            return completions;
        }

        let line = text.line(line_idx);
        let line_str = line.to_string();
        let prefix = if char_idx <= line_str.len() {
            &line_str[..char_idx]
        } else {
            &line_str
        };

        // Check if we're inside a plugin block
        let text_str = text.to_string();
        if let Some(block_name) = self.detect_plugin_block_context(&text_str, line_idx) {
            // Get plugin-specific completions from WASM plugins
            completions.extend(self.get_plugin_completions(&block_name, &text_str, line_idx, char_idx, prefix));
            // Get completions from static language definitions
            completions.extend(self.get_language_registry_completions(&block_name, prefix));
        }

        // Get completions from language registry based on context
        completions.extend(self.get_context_aware_completions(prefix));

        // Check for different completion contexts
        if self.is_after_dot(prefix) {
            // Method completion after dot (e.g., "string.len")
            completions.extend(self.get_method_completions(prefix));
        } else if self.is_apply_block_context(prefix) {
            // Apply-block completions (e.g., after "identifier:")
            completions.extend(self.get_apply_block_completions());
            // Also add plugin block completions
            completions.extend(self.get_plugin_block_completions());
        } else if self.is_type_context(prefix) {
            // Type completions (e.g., function parameters, variable declarations)
            completions.extend(self.get_type_completions());
        } else if self.is_function_context(prefix) {
            // Function-related completions
            completions.extend(self.get_function_completions());
        } else {
            // General keyword and language construct completions
            completions.extend(self.get_keyword_completions(prefix));
            completions.extend(self.get_builtin_function_completions());
            // Add plugin block completions at top level too
            completions.extend(self.get_plugin_block_completions());
        }

        completions
    }

    /// Detect if we're inside a plugin block and return the block name
    fn detect_plugin_block_context(&self, text: &str, current_line: usize) -> Option<String> {
        let registry = self.plugin_registry.as_ref()?;
        let lines: Vec<&str> = text.lines().collect();

        // Search backwards from current line to find a block declaration
        for i in (0..=current_line).rev() {
            if i >= lines.len() {
                continue;
            }
            let line = lines[i].trim();

            // Check if this line starts a block
            if line.ends_with(':') && !line.starts_with('#') {
                let block_name = line.trim_end_matches(':');
                if registry.handles(block_name) {
                    return Some(block_name.to_string());
                }
            }

            // If we hit an unindented non-empty line that isn't a block, we're not in a block
            if !lines[i].starts_with('\t') && !lines[i].starts_with("    ") && !line.is_empty() {
                // Unless this is the block declaration itself
                if !line.ends_with(':') {
                    return None;
                }
            }
        }

        None
    }

    /// Get completions from plugins for the current block context
    fn get_plugin_completions(
        &self,
        block_name: &str,
        text: &str,
        line: usize,
        column: usize,
        prefix: &str,
    ) -> Vec<CompletionItem> {
        let registry = match &self.plugin_registry {
            Some(r) => r,
            None => return Vec::new(),
        };

        let ctx = PluginLspContext::new(block_name, text, prefix)
            .at_position(line, column);

        let plugin_completions = registry.get_completions(block_name, &ctx);
        plugin_completions
            .into_iter()
            .map(|item| self.convert_plugin_completion(item))
            .collect()
    }

    /// Get block-level completions from all plugins
    fn get_plugin_block_completions(&self) -> Vec<CompletionItem> {
        let registry = match &self.plugin_registry {
            Some(r) => r,
            None => return Vec::new(),
        };

        registry
            .get_block_completions()
            .into_iter()
            .map(|item| self.convert_plugin_completion(item))
            .collect()
    }

    /// Convert a plugin completion item to an LSP completion item
    /// Plugin items use MODULE icon (purple) to differentiate from built-in FUNCTION (blue)
    fn convert_plugin_completion(&self, item: PluginCompletionItem) -> CompletionItem {
        CompletionItem {
            label: item.label,
            // Use MODULE for plugin functions (purple icon) vs FUNCTION for built-ins (blue icon)
            kind: Some(match item.kind {
                PluginCompletionKind::Keyword => CompletionItemKind::KEYWORD,
                PluginCompletionKind::Function => CompletionItemKind::MODULE, // Purple icon for plugin functions
                PluginCompletionKind::Snippet => CompletionItemKind::SNIPPET,
                PluginCompletionKind::Type => CompletionItemKind::TYPE_PARAMETER,
                PluginCompletionKind::Property => CompletionItemKind::PROPERTY,
                PluginCompletionKind::Variable => CompletionItemKind::VARIABLE,
                PluginCompletionKind::Operator => CompletionItemKind::OPERATOR,
            }),
            detail: item.detail,
            documentation: item.documentation.map(Documentation::String),
            insert_text: item.insert_text,
            insert_text_format: if item.is_snippet {
                Some(InsertTextFormat::SNIPPET)
            } else {
                Some(InsertTextFormat::PLAIN_TEXT)
            },
            ..Default::default()
        }
    }

    /// Get completions from static language registry definitions for a specific block
    fn get_language_registry_completions(
        &self,
        _block_name: &str,
        prefix: &str,
    ) -> Vec<CompletionItem> {
        let registry = match &self.language_registry {
            Some(r) => r,
            None => return Vec::new(),
        };

        let mut completions = Vec::new();

        // Get keywords for this block context
        for keyword in registry.keywords_for_context("block") {
            if keyword.name.to_lowercase().starts_with(&prefix.to_lowercase()) {
                completions.push(CompletionItem {
                    label: keyword.name.clone(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some(format!("Keyword ({})", keyword.plugin_name)),
                    documentation: Some(Documentation::String(keyword.description.clone())),
                    insert_text: Some(keyword.name.clone()),
                    ..Default::default()
                });
            }
        }

        // Get types from the registry
        for type_name in registry.all_types() {
            if type_name.to_lowercase().starts_with(&prefix.to_lowercase()) {
                if let Some(type_info) = registry.get_type(type_name) {
                    completions.push(CompletionItem {
                        label: type_name.to_string(),
                        kind: Some(CompletionItemKind::TYPE_PARAMETER),
                        detail: Some(format!("Type ({})", type_info.plugin_name)),
                        documentation: Some(Documentation::String(type_info.description.clone())),
                        insert_text: Some(type_name.to_string()),
                        ..Default::default()
                    });
                }
            }
        }

        // Get functions from the registry (plugin functions use MODULE icon - purple)
        for func_name in registry.all_functions() {
            if func_name.to_lowercase().starts_with(&prefix.to_lowercase()) {
                if let Some(func_info) = registry.get_function(func_name) {
                    completions.push(CompletionItem {
                        label: func_name.to_string(),
                        kind: Some(CompletionItemKind::MODULE), // Purple icon for plugin functions
                        detail: Some(func_info.signature.clone()),
                        documentation: Some(Documentation::String(format!(
                            "{}\n\n📦 Plugin: {}",
                            func_info.description,
                            func_info.plugin_name
                        ))),
                        insert_text: Some(format!("{}()", func_name)),
                        ..Default::default()
                    });
                }
            }
        }

        // Get snippet completions
        for snippet in registry.get_completions(prefix) {
            completions.push(CompletionItem {
                label: snippet.trigger.clone(),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: snippet.description.clone(),
                documentation: Some(Documentation::String(format!(
                    "Plugin: {}",
                    snippet.plugin_name
                ))),
                insert_text: Some(snippet.insert.clone()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
        }

        completions
    }

    /// Get context-aware completions from language registry
    fn get_context_aware_completions(&self, prefix: &str) -> Vec<CompletionItem> {
        let registry = match &self.language_registry {
            Some(r) => r,
            None => return Vec::new(),
        };

        let mut completions = Vec::new();

        // Add block-level completions (e.g., "data:", "endpoints:") - plugin blocks use MODULE icon
        for block_name in registry.all_blocks() {
            if block_name.to_lowercase().starts_with(&prefix.to_lowercase()) {
                if let Some(block_info) = registry.get_block(block_name) {
                    completions.push(CompletionItem {
                        label: format!("{}:", block_name),
                        kind: Some(CompletionItemKind::MODULE), // Purple icon for plugin blocks
                        detail: Some(format!("📦 {}", block_info.plugin_name)),
                        documentation: block_info
                            .description
                            .as_ref()
                            .map(|d| Documentation::String(format!("{}\n\n📦 Plugin: {}", d, block_info.plugin_name))),
                        insert_text: Some(format!("{}:\n\t${{1:content}}", block_name)),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    });
                }
            }
        }

        // Add expression-context keywords
        for keyword in registry.keywords_for_context("expression") {
            if keyword.name.to_lowercase().starts_with(&prefix.to_lowercase()) {
                completions.push(CompletionItem {
                    label: keyword.name.clone(),
                    kind: Some(CompletionItemKind::KEYWORD),
                    detail: Some(format!("Keyword ({})", keyword.plugin_name)),
                    documentation: Some(Documentation::String(keyword.description.clone())),
                    insert_text: Some(keyword.name.clone()),
                    ..Default::default()
                });
            }
        }

        completions
    }

    fn is_after_dot(&self, prefix: &str) -> bool {
        prefix.trim_end().ends_with('.')
    }

    fn is_apply_block_context(&self, prefix: &str) -> bool {
        prefix.trim_end().ends_with(':')
    }

    fn is_type_context(&self, prefix: &str) -> bool {
        // Check if we're in a position where type annotation is expected
        prefix.contains("(") || prefix.contains("->") || prefix.contains(":")
    }

    fn is_function_context(&self, prefix: &str) -> bool {
        prefix.trim().is_empty() || prefix.trim() == "functions"
    }

    fn get_method_completions(&self, prefix: &str) -> Vec<CompletionItem> {
        let mut completions = Vec::new();

        // Determine the object type from context (simplified)
        if prefix.contains("string") || prefix.contains("\"") {
            completions.extend(self.get_string_methods());
        } else if prefix.contains("integer") || prefix.contains("number") {
            completions.extend(self.get_number_methods());
        } else if prefix.contains("list") || prefix.contains("[") {
            completions.extend(self.get_list_methods());
        } else {
            // Generic object methods
            completions.extend(self.get_generic_methods());
        }

        completions
    }

    fn get_string_methods(&self) -> Vec<CompletionItem> {
        vec![
            self.create_method_completion("length", "Get the length of the string", "property"),
            self.create_method_completion("charAt", "Get character at index", "charAt(${1:index})"),
            self.create_method_completion(
                "substring",
                "Extract substring",
                "substring(${1:start}, ${2:end})",
            ),
            self.create_method_completion(
                "indexOf",
                "Find index of substring",
                "indexOf(${1:substring})",
            ),
            self.create_method_completion(
                "replace",
                "Replace occurrences",
                "replace(${1:search}, ${2:replacement})",
            ),
            self.create_method_completion("toUpperCase", "Convert to uppercase", "toUpperCase()"),
            self.create_method_completion("toLowerCase", "Convert to lowercase", "toLowerCase()"),
            self.create_method_completion("trim", "Remove whitespace", "trim()"),
            self.create_method_completion("split", "Split string", "split(${1:delimiter})"),
        ]
    }

    fn get_number_methods(&self) -> Vec<CompletionItem> {
        vec![
            self.create_method_completion("toString", "Convert to string", "toString()"),
            self.create_method_completion("abs", "Absolute value", "abs()"),
            self.create_method_completion("round", "Round to nearest integer", "round()"),
            self.create_method_completion("floor", "Round down", "floor()"),
            self.create_method_completion("ceil", "Round up", "ceil()"),
        ]
    }

    fn get_list_methods(&self) -> Vec<CompletionItem> {
        vec![
            self.create_method_completion("length", "Get the length of the list", "property"),
            self.create_method_completion("push", "Add element to end", "push(${1:element})"),
            self.create_method_completion("pop", "Remove and return last element", "pop()"),
            self.create_method_completion("get", "Get element at index", "get(${1:index})"),
            self.create_method_completion(
                "set",
                "Set element at index",
                "set(${1:index}, ${2:value})",
            ),
            self.create_method_completion(
                "indexOf",
                "Find index of element",
                "indexOf(${1:element})",
            ),
            self.create_method_completion(
                "contains",
                "Check if contains element",
                "contains(${1:element})",
            ),
            self.create_method_completion("clear", "Remove all elements", "clear()"),
            self.create_method_completion(
                "slice",
                "Extract portion of list",
                "slice(${1:start}, ${2:end})",
            ),
        ]
    }

    fn get_generic_methods(&self) -> Vec<CompletionItem> {
        vec![
            self.create_method_completion("toString", "Convert to string", "toString()"),
            self.create_method_completion("equals", "Check equality", "equals(${1:other})"),
            self.create_method_completion("hashCode", "Get hash code", "hashCode()"),
        ]
    }

    fn get_apply_block_completions(&self) -> Vec<CompletionItem> {
        vec![
            self.create_snippet_completion(
                "apply_function",
                "Function call with apply block",
                "\n\t${1:function_call}\n\t${2:another_call}",
            ),
            self.create_snippet_completion(
                "apply_assignment",
                "Variable assignments",
                "\n\t${1:variable} = ${2:value}\n\t${3:another_var} = ${4:value}",
            ),
            self.create_snippet_completion(
                "apply_method_chain",
                "Method chain calls",
                "\n\t${1:method}(${2:args})\n\t${3:method}(${4:args})",
            ),
        ]
    }

    fn get_type_completions(&self) -> Vec<CompletionItem> {
        vec![
            self.create_type_completion("integer", "Integer type"),
            self.create_type_completion("number", "Number/float type"),
            self.create_type_completion("string", "String type"),
            self.create_type_completion("boolean", "Boolean type"),
            self.create_type_completion("void", "Void type (no return value)"),
            self.create_type_completion("any", "Any type"),
            self.create_snippet_completion("list_type", "List type", "list<${1:element_type}>"),
            self.create_snippet_completion(
                "matrix_type",
                "Matrix type",
                "matrix<${1:element_type}>",
            ),
            self.create_snippet_completion(
                "pairs_type",
                "Pairs type",
                "pairs<${1:key_type}, ${2:value_type}>",
            ),
        ]
    }

    fn get_function_completions(&self) -> Vec<CompletionItem> {
        vec![
            self.create_snippet_completion(
                "function",
                "Function declaration",
                "${1:return_type} ${2:function_name}(${3:parameters})\n\t${4:body}",
            ),
            self.create_snippet_completion(
                "void_function",
                "Void function",
                "${1:function_name}(${2:parameters})\n\t${3:body}",
            ),
            self.create_snippet_completion(
                "start_function",
                "Start function",
                "start()\n\t${1:body}",
            ),
        ]
    }

    fn get_keyword_completions(&self, prefix: &str) -> Vec<CompletionItem> {
        let keywords = [
            (
                "functions",
                "Functions block",
                "functions:\n\t${1:function_definitions}",
            ),
            (
                "class",
                "Class declaration",
                "class ${1:ClassName}\n\t${2:body}",
            ),
            (
                "constants",
                "Constants block",
                "constants:\n\t${1:constant_definitions}",
            ),
            ("types", "Types block", "types:\n\t${1:type_definitions}"),
            ("start", "Start function", "start()\n\t${1:body}"),
            ("if", "If statement", "if ${1:condition}\n\t${2:body}"),
            ("else", "Else statement", "else\n\t${1:body}"),
            ("while", "While loop", "while ${1:condition}\n\t${2:body}"),
            (
                "for",
                "For loop",
                "for ${1:variable} in ${2:iterable}\n\t${3:body}",
            ),
            ("return", "Return statement", "return ${1:value}"),
            ("extends", "Class inheritance", "extends ${1:BaseClass}"),
            ("base", "Base constructor call", "base(${1:arguments})"),
            (
                "onError",
                "Error handling",
                "onError ${1:error_variable}\n\t${2:error_handling}",
            ),
        ];

        keywords
            .iter()
            .filter(|(keyword, _, _)| {
                prefix.is_empty() || keyword.starts_with(&prefix.to_lowercase())
            })
            .map(|(keyword, description, snippet)| CompletionItem {
                label: keyword.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(description.to_string()),
                documentation: Some(Documentation::String(format!(
                    "Clean Language keyword: {description}"
                ))),
                insert_text: Some(snippet.to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            })
            .collect()
    }

    fn get_builtin_function_completions(&self) -> Vec<CompletionItem> {
        vec![
            self.create_function_completion("print", "Print to console (use + for newline)", "print(${1:message})"),
            self.create_function_completion("input", "Get user input", "input(${1:prompt})"),
            self.create_function_completion("error", "Print error message", "error(${1:message})"),
        ]
    }

    fn create_method_completion(
        &self,
        name: &str,
        description: &str,
        insert_text: &str,
    ) -> CompletionItem {
        CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::METHOD),
            detail: Some(description.to_string()),
            documentation: Some(Documentation::String(format!(
                "Method: {name}\n\n{description}"
            ))),
            insert_text: if insert_text == "property" {
                Some(name.to_string())
            } else {
                Some(insert_text.to_string())
            },
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        }
    }

    fn create_type_completion(&self, name: &str, description: &str) -> CompletionItem {
        CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            detail: Some(description.to_string()),
            documentation: Some(Documentation::String(format!(
                "Clean Language type: {description}"
            ))),
            insert_text: Some(name.to_string()),
            ..Default::default()
        }
    }

    fn create_function_completion(
        &self,
        name: &str,
        description: &str,
        insert_text: &str,
    ) -> CompletionItem {
        CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(description.to_string()),
            documentation: Some(Documentation::String(format!(
                "Built-in function: {name}\n\n{description}"
            ))),
            insert_text: Some(insert_text.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        }
    }

    fn create_snippet_completion(
        &self,
        name: &str,
        description: &str,
        insert_text: &str,
    ) -> CompletionItem {
        CompletionItem {
            label: name.to_string(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some(description.to_string()),
            documentation: Some(Documentation::String(format!(
                "Code snippet: {description}"
            ))),
            insert_text: Some(insert_text.to_string()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        }
    }
}

impl Default for CompletionProvider {
    fn default() -> Self {
        Self::new()
    }
}
