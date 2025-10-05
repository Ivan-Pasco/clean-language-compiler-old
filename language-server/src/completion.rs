/*
 * Clean Language Server - Completion Provider
 *
 * Provides intelligent autocompletion for Clean Language including:
 * - Keywords and syntax patterns
 * - Built-in functions and methods
 * - Type annotations
 * - Apply-block completions
 * - Language constructs
 */

use ropey::Rope;
use tower_lsp::lsp_types::*;

pub struct CompletionProvider;

impl CompletionProvider {
    pub fn new() -> Self {
        Self
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

        // Check for different completion contexts
        if self.is_after_dot(prefix) {
            // Method completion after dot (e.g., "string.len")
            completions.extend(self.get_method_completions(prefix));
        } else if self.is_apply_block_context(prefix) {
            // Apply-block completions (e.g., after "identifier:")
            completions.extend(self.get_apply_block_completions());
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
            self.create_function_completion("print", "Print to console", "print(${1:message})"),
            self.create_function_completion(
                "println",
                "Print line to console",
                "println(${1:message})",
            ),
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
