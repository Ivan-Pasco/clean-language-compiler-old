/*
 * Clean Language Server - Hover Provider
 *
 * Provides hover information and documentation for Clean Language elements
 * including plugin-provided documentation for DSL blocks.
 */

use clean_language_compiler::plugins::{LanguageRegistry, PluginRegistry};
use ropey::Rope;
use std::sync::Arc;
use tower_lsp::lsp_types::*;

pub struct HoverProvider {
    /// Plugin registry for dynamic hover information (WASM plugins)
    plugin_registry: Option<Arc<PluginRegistry>>,
    /// Language registry for static hover information (plugin.toml definitions)
    language_registry: Option<Arc<LanguageRegistry>>,
}

impl HoverProvider {
    pub fn new() -> Self {
        Self {
            plugin_registry: None,
            language_registry: None,
        }
    }

    /// Create a hover provider with both plugin and language registries
    pub fn with_language_registry(
        plugin_registry: Arc<PluginRegistry>,
        language_registry: Arc<LanguageRegistry>,
    ) -> Self {
        Self {
            plugin_registry: Some(plugin_registry),
            language_registry: Some(language_registry),
        }
    }

    pub async fn provide_hover(&self, text: &Rope, position: Position) -> Option<Hover> {
        let line_idx = position.line as usize;

        if line_idx >= text.len_lines() {
            return None;
        }

        let line = text.line(line_idx);
        let line_str = line.to_string();

        // Extract word at position
        let char_idx = position.character as usize;
        let word = self.extract_word_at_position(&line_str, char_idx)?;

        // Provide hover information for different language elements
        self.get_hover_info(&word, &line_str, text)
    }

    fn extract_word_at_position(&self, line: &str, char_idx: usize) -> Option<String> {
        if char_idx >= line.len() {
            return None;
        }

        let chars: Vec<char> = line.chars().collect();
        let mut start = char_idx;
        let mut end = char_idx;

        // Find the start of the word
        while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
            start -= 1;
        }

        // Find the end of the word
        while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
            end += 1;
        }

        if start < end {
            Some(chars[start..end].iter().collect())
        } else {
            None
        }
    }

    fn get_hover_info(&self, word: &str, line: &str, text: &Rope) -> Option<Hover> {
        // Check different categories of language elements

        // First check WASM plugins - they may provide hover for their keywords
        if let Some(plugin_info) = self.get_plugin_hover_info(word, text) {
            return Some(self.create_hover(plugin_info));
        }

        // Check static language registry definitions
        if let Some(registry_info) = self.get_registry_hover(word) {
            return Some(self.create_hover(registry_info));
        }

        // Keywords
        if let Some(keyword_info) = self.get_keyword_info(word) {
            return Some(self.create_hover(keyword_info));
        }

        // Types
        if let Some(type_info) = self.get_type_info(word) {
            return Some(self.create_hover(type_info));
        }

        // Built-in functions
        if let Some(builtin_info) = self.get_builtin_function_info(word, line) {
            return Some(self.create_hover(builtin_info));
        }

        // Method information (after dot)
        if line.contains(&format!(".{word}")) {
            if let Some(method_info) = self.get_method_info(word, line) {
                return Some(self.create_hover(method_info));
            }
        }

        // Language constructs
        if let Some(construct_info) = self.get_construct_info(word, line) {
            return Some(self.create_hover(construct_info));
        }

        None
    }

    /// Get hover information from static language registry definitions
    fn get_registry_hover(&self, word: &str) -> Option<String> {
        let registry = self.language_registry.as_ref()?;

        // Check if it's a block name
        if let Some(block_info) = registry.get_block(word) {
            return Some(format!(
                "**📦 {}:** Block\n\n{}\n\n---\n📦 *Plugin: {}*",
                word,
                block_info.description.as_deref().unwrap_or("Plugin-defined block"),
                block_info.plugin_name
            ));
        }

        // Check if it's a keyword
        if let Some(keyword_info) = registry.get_keyword(word) {
            return Some(format!(
                "**📦 {}** Keyword\n\n{}\n\n*Context: {}*\n\n---\n📦 *Plugin: {}*",
                word,
                keyword_info.description,
                keyword_info.context,
                keyword_info.plugin_name
            ));
        }

        // Check if it's a type
        if let Some(type_info) = registry.get_type(word) {
            return Some(format!(
                "**📦 {}** Type\n\n{}\n\n---\n📦 *Plugin: {}*",
                word, type_info.description, type_info.plugin_name
            ));
        }

        // Check if it's a function
        if let Some(func_info) = registry.get_function(word) {
            return Some(format!(
                "**📦 {}** Function\n\n```clean\n{}\n```\n\n{}\n\n---\n📦 *Plugin: {}*",
                word, func_info.signature, func_info.description, func_info.plugin_name
            ));
        }

        None
    }

    /// Get hover information from plugins
    fn get_plugin_hover_info(&self, word: &str, text: &Rope) -> Option<String> {
        let registry = self.plugin_registry.as_ref()?;

        // Detect if we're inside a plugin block
        let text_str = text.to_string();
        let block_name = self.detect_plugin_block_context(&text_str);

        // Get hover info from the registry
        if let Some(info) = registry.get_hover_info(word, block_name.as_deref()) {
            return Some(info.content);
        }

        None
    }

    /// Detect if we're inside a plugin block and return the block name
    fn detect_plugin_block_context(&self, text: &str) -> Option<String> {
        let registry = self.plugin_registry.as_ref()?;
        let lines: Vec<&str> = text.lines().collect();

        // Look for a plugin block in the document
        for line in &lines {
            let trimmed = line.trim();
            if trimmed.ends_with(':') && !trimmed.starts_with('#') {
                let block_name = trimmed.trim_end_matches(':');
                if registry.handles(block_name) {
                    return Some(block_name.to_string());
                }
            }
        }

        None
    }

    fn get_keyword_info(&self, word: &str) -> Option<String> {
        match word {
            "functions" => Some("**functions:** Block\n\nDefines a block containing function declarations.\n\n```clean\nfunctions:\n\tinteger add(integer a, integer b)\n\t\treturn a + b\n```\n\n---\n⚡ *Built-in*".to_string()),
            "class" => Some("**class** Keyword\n\nDefines a class with optional inheritance.\n\n```clean\nclass MyClass extends BaseClass\n\tfield: string\n\tconstructor(value: string)\n\t\tfield = value\n```\n\n---\n⚡ *Built-in*".to_string()),
            "start" => Some("**start()** Function\n\nEntry point function for Clean Language programs.\n\n```clean\nstart()\n\tprint(\"Hello, World!\")\n```\n\n---\n⚡ *Built-in*".to_string()),
            "if" => Some("**if** Statement\n\nConditional execution.\n\n```clean\nif condition\n\t// code block\nelse\n\t// alternative block\n```\n\n---\n⚡ *Built-in*".to_string()),
            "while" => Some("**while** Loop\n\nRepeats a block while condition is true.\n\n```clean\nwhile count < 10\n\tprint(count)\n\tcount = count + 1\n```\n\n---\n⚡ *Built-in*".to_string()),
            "for" => Some("**for** Loop\n\nIterates over collections.\n\n```clean\nfor item in collection\n\tprint(item)\n```\n\n---\n⚡ *Built-in*".to_string()),
            "return" => Some("**return** Statement\n\nReturns a value from a function.\n\n```clean\nreturn value\n```\n\n---\n⚡ *Built-in*".to_string()),
            "constants" => Some("**constants:** Block\n\nDefines constant values.\n\n```clean\nconstants:\n\tPI = 3.14159\n\tMAX_SIZE = 100\n```\n\n---\n⚡ *Built-in*".to_string()),
            "types" => Some("**types:** Block\n\nDefines custom type aliases.\n\n```clean\ntypes:\n\tUserId = integer\n\tUserName = string\n```\n\n---\n⚡ *Built-in*".to_string()),
            "extends" => Some("**extends** Keyword\n\nUsed in class inheritance.\n\n```clean\nclass Child extends Parent\n\t// child class body\n```\n\n---\n⚡ *Built-in*".to_string()),
            "base" => Some("**base()** Call\n\nCalls the parent class constructor.\n\n```clean\nconstructor(value: string)\n\tbase(value)\n```\n\n---\n⚡ *Built-in*".to_string()),
            "onError" => Some("**onError** Handler\n\nError handling mechanism.\n\n```clean\nonError error\n\tprint(\"Error occurred: \" + error)\n```\n\n---\n⚡ *Built-in*".to_string()),
            _ => None,
        }
    }

    fn get_type_info(&self, word: &str) -> Option<String> {
        match word {
            "integer" => Some("**integer** Type\n\nSigned integer number type.\n\n**Examples:**\n```clean\ninteger count = 42\ninteger negative = -10\n```\n\n**Methods:**\n- `toString()` - Convert to string\n- `abs()` - Absolute value\n\n---\n⚡ *Built-in*".to_string()),
            "number" => Some("**number** Type\n\nFloating-point number type.\n\n**Examples:**\n```clean\nnumber pi = 3.14159\nnumber temperature = -5.5\n```\n\n**Methods:**\n- `toString()` - Convert to string\n- `round()` - Round to nearest integer\n- `floor()` - Round down\n- `ceil()` - Round up\n\n---\n⚡ *Built-in*".to_string()),
            "string" => Some("**string** Type\n\nText string type.\n\n**Examples:**\n```clean\nstring name = \"Alice\"\nstring greeting = \"Hello, \" + name\n```\n\n**Methods:**\n- `length` - Get string length\n- `charAt(index)` - Get character at index\n- `substring(start, end)` - Extract substring\n- `indexOf(text)` - Find text position\n- `replace(old, new)` - Replace text\n\n---\n⚡ *Built-in*".to_string()),
            "boolean" => Some("**boolean** Type\n\nBoolean true/false type.\n\n**Examples:**\n```clean\nboolean isValid = true\nboolean isEmpty = false\n```\n\n**Values:**\n- `true`\n- `false`\n\n---\n⚡ *Built-in*".to_string()),
            "void" => Some("**void** Type\n\nRepresents no return value.\n\n**Usage:**\n```clean\nvoid printMessage(string msg)\n\tprint(msg)\n\t// no return statement needed\n```\n\n---\n⚡ *Built-in*".to_string()),
            "any" => Some("**any** Type\n\nAccepts any type of value.\n\n**Usage:**\n```clean\nany value = 42\nvalue = \"text\"\nvalue = true\n```\n\n**Note:** Use sparingly for type safety.\n\n---\n⚡ *Built-in*".to_string()),
            "list" => Some("**list<T>** Type\n\nDynamic array type.\n\n**Examples:**\n```clean\nlist<integer> numbers = [1, 2, 3]\nlist<string> names = [\"Alice\", \"Bob\"]\n```\n\n**Methods:**\n- `length` - Get list size\n- `push(item)` - Add item to end\n- `pop()` - Remove last item\n- `get(index)` - Get item at index\n\n---\n⚡ *Built-in*".to_string()),
            "matrix" => Some("**matrix<T>** Type\n\nTwo-dimensional array type.\n\n**Examples:**\n```clean\nmatrix<integer> grid = [[1, 2], [3, 4]]\n```\n\n**Usage:**\n- Multi-dimensional data\n- Mathematical operations\n\n---\n⚡ *Built-in*".to_string()),
            _ => None,
        }
    }

    fn get_builtin_function_info(&self, word: &str, _line: &str) -> Option<String> {
        match word {
            "print" => Some("**print()** Function\n\nPrints a message to the console.\n\n**Signature:**\n```clean\nprint(message: any)      // no newline\nprint(message: any) +    // with newline\n```\n\n**Examples:**\n```clean\nprint(\"Hello\")           // no newline\nprint(\"Hello, World!\") + // with newline\nprint(42) +\nprint(variable) +\n```\n\n---\n⚡ *Built-in*".to_string()),
            "input" => Some("**input()** Function\n\nGets user input from console.\n\n**Signature:**\n```clean\ninput(prompt: string) -> string\n```\n\n**Example:**\n```clean\nstring name = input(\"Enter your name: \")\nprint(\"Hello, \" + name)\n```\n\n---\n⚡ *Built-in*".to_string()),
            "error" => Some("**error()** Function\n\nPrints an error message.\n\n**Signature:**\n```clean\nerror(message: string)\n```\n\n**Example:**\n```clean\nerror(\"Something went wrong!\")\n```\n\n---\n⚡ *Built-in*".to_string()),
            _ => None,
        }
    }

    fn get_method_info(&self, word: &str, line: &str) -> Option<String> {
        // Determine context from the line
        if line.contains("string.") || line.contains("\".") {
            self.get_string_method_info(word)
        } else if line.contains("list.") || line.contains("[.") {
            self.get_list_method_info(word)
        } else if line.contains("number.") || line.contains("integer.") {
            self.get_number_method_info(word)
        } else {
            None
        }
    }

    fn get_string_method_info(&self, word: &str) -> Option<String> {
        match word {
            "length" => Some("**string.length** Property\n\nGets the length of the string.\n\n**Type:** `integer`\n\n**Example:**\n```clean\nstring text = \"Hello\"\ninteger len = text.length  // 5\n```".to_string()),
            "charAt" => Some("**string.charAt()** Method\n\nGets the character at the specified index.\n\n**Signature:**\n```clean\ncharAt(index: integer) -> string\n```\n\n**Example:**\n```clean\nstring text = \"Hello\"\nstring char = text.charAt(0)  // \"H\"\n```".to_string()),
            "substring" => Some("**string.substring()** Method\n\nExtracts a portion of the string.\n\n**Signature:**\n```clean\nsubstring(start: integer, end: integer) -> string\n```\n\n**Example:**\n```clean\nstring text = \"Hello World\"\nstring sub = text.substring(0, 5)  // \"Hello\"\n```".to_string()),
            _ => None,
        }
    }

    fn get_list_method_info(&self, word: &str) -> Option<String> {
        match word {
            "length" => Some("**list.length** Property\n\nGets the number of items in the list.\n\n**Type:** `integer`\n\n**Example:**\n```clean\nlist<integer> nums = [1, 2, 3]\ninteger count = nums.length  // 3\n```".to_string()),
            "push" => Some("**list.push()** Method\n\nAdds an item to the end of the list.\n\n**Signature:**\n```clean\npush(item: T) -> void\n```\n\n**Example:**\n```clean\nlist<integer> nums = [1, 2]\nnums.push(3)  // [1, 2, 3]\n```".to_string()),
            "get" => Some("**list.get()** Method\n\nGets the item at the specified index.\n\n**Signature:**\n```clean\nget(index: integer) -> T\n```\n\n**Example:**\n```clean\nlist<string> names = [\"Alice\", \"Bob\"]\nstring first = names.get(0)  // \"Alice\"\n```".to_string()),
            _ => None,
        }
    }

    fn get_number_method_info(&self, word: &str) -> Option<String> {
        match word {
            "toString" => Some("**number.toString()** Method\n\nConverts the number to a string.\n\n**Signature:**\n```clean\ntoString() -> string\n```\n\n**Example:**\n```clean\nnumber value = 42.5\nstring text = value.toString()  // \"42.5\"\n```".to_string()),
            "round" => Some("**number.round()** Method\n\nRounds to the nearest integer.\n\n**Signature:**\n```clean\nround() -> integer\n```\n\n**Example:**\n```clean\nnumber value = 42.7\ninteger rounded = value.round()  // 43\n```".to_string()),
            _ => None,
        }
    }

    fn get_construct_info(&self, word: &str, line: &str) -> Option<String> {
        // Check for apply-block patterns
        if line.contains(&format!("{word}:")) {
            Some(format!("**{word}:** Apply Block\n\nApplies the identifier '{word}' to each indented item below.\n\n**Pattern:**\n```clean\n{word}:\n\titem1\n\titem2\n\titem3\n```\n\n**Usage:**\n- Function calls\n- Variable assignments\n- Method chains"))
        } else {
            None
        }
    }

    fn create_hover(&self, content: String) -> Hover {
        Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
            }),
            range: None,
        }
    }
}

impl Default for HoverProvider {
    fn default() -> Self {
        Self::new()
    }
}
