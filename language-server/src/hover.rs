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
            "class" => Some("**class** Keyword\n\nDefines a class with optional inheritance.\n\n```clean\nclass Person\n\tstring name\n\tinteger age\n\n\tconstructor(string nameParam, integer ageParam)\n\t\tname = nameParam\n\t\tage = ageParam\n\n\tfunctions:\n\t\tstring greet()\n\t\t\treturn \"I'm \" + name\n```\n\nInheritance uses `is`:\n```clean\nclass Dog is Animal\n\tstring breed\n\n\tconstructor(string nameParam, string breedParam)\n\t\tbase(nameParam)\n\t\tbreed = breedParam\n```\n\n---\n⚡ *Built-in*".to_string()),
            "start" => Some("**start:** Block\n\nEntry point block for Clean Language programs.\n\n```clean\nstart:\n\tprint(\"Hello, World!\") +\n```\n\n---\n⚡ *Built-in*".to_string()),
            "if" => Some("**if** Statement\n\nConditional execution.\n\n```clean\nif condition\n\t// code block\nelse\n\t// alternative block\n```\n\n---\n⚡ *Built-in*".to_string()),
            "iterate" => Some("**iterate** Loop\n\nIterates over collections or ranges.\n\n```clean\niterate item in myList\n\tprint(item.toString()) +\n\niterate i in 1 to 10\n\tprint(i.toString()) +\n\niterate k in 10 to 1 step -2\n\tprint(k.toString()) +\n```\n\n---\n⚡ *Built-in*".to_string()),
            "repeat" => Some("**repeat** Loop\n\nRepeats a block indefinitely until `break`.\n\n```clean\nrepeat\n\tstring input = input(\"Continue? \")\n\tif input == \"no\"\n\t\tbreak\n```\n\n---\n⚡ *Built-in*".to_string()),
            "return" => Some("**return** Statement\n\nReturns a value from a function.\n\n```clean\nreturn value\n```\n\n---\n⚡ *Built-in*".to_string()),
            "constants" => Some("**constants:** Block\n\nDefines constant values.\n\n```clean\nconstants:\n\tPI = 3.14159\n\tMAX_SIZE = 100\n```\n\n---\n⚡ *Built-in*".to_string()),
            "types" => Some("**types:** Block\n\nDefines custom type aliases.\n\n```clean\ntypes:\n\tUserId = integer\n\tUserName = string\n```\n\n---\n⚡ *Built-in*".to_string()),
            "is" => Some("**is** Keyword\n\nUsed for class inheritance. Declares that a class extends a base class.\n\n```clean\nclass Dog is Animal\n\tstring breed\n\n\tconstructor(string nameParam, string breedParam)\n\t\tbase(nameParam)\n\t\tbreed = breedParam\n\n\tfunctions:\n\t\tstring speak()\n\t\t\treturn name + \" barks\"\n```\n\n---\n⚡ *Built-in*".to_string()),
            "base" => Some("**base()** Call\n\nCalls the parent class constructor.\n\n```clean\nconstructor(value: string)\n\tbase(value)\n```\n\n---\n⚡ *Built-in*".to_string()),
            "onError" => Some("**onError** Operator\n\nInline error handler — provides a fallback value if the expression throws.\n\n```clean\nstart:\n\tinteger result = divide(10, 0) onError 0\n\tstring name = readFile(\"config.txt\") onError \"default\"\n```\n\n---\n⚡ *Built-in*".to_string()),
            "state" => Some("**state:** Block\n\nDeclares reactive state variables.\n\n```clean\nstate:\n\tinteger counter = 0\n\tstring name = \"default\"\n```\n\n---\n⚡ *Built-in*".to_string()),
            "watch" => Some("**watch** Block\n\nReacts to state variable changes.\n\n```clean\nwatch counter:\n\tprint(\"Counter changed!\") +\n```\n\n---\n⚡ *Built-in*".to_string()),
            "computed" => Some("**computed:** Block\n\nDerived values that recalculate when dependencies change. Used inside `state:` or inside a `class`.\n\n```clean\nstate:\n\tinteger count = 0\n\n\tcomputed:\n\t\tstring display\n\t\t\treturn \"Count: \" + count.toString()\n```\n\nInside a class:\n```clean\nclass Circle\n\tnumber radius\n\n\tcomputed:\n\t\tnumber area = 3.14159 * radius * radius\n\t\tnumber circumference = 2.0 * 3.14159 * radius\n```\n\n---\n⚡ *Built-in*".to_string()),
            "rules" => Some("**rules:** Block\n\nDefines validation constraints for state variables.\n\n```clean\nrules:\n\tage >= 0\n\tname != \"\"\n```\n\n---\n⚡ *Built-in*".to_string()),
            "tests" => Some("**tests:** Block\n\nInline test assertions — two forms:\n\n**Expression tests** (pure functions, run with `cln test`):\n```clean\ntests:\n\t\"addition works\": 2 + 2 = 4\n\t\"string length\": string.length(\"hello\") = 5\n\tadd(2, 3) = 5\n```\n\n**Endpoint tests** (HTTP, requires `cleen serve`):\n```clean\ntests:\n\ttest \"health check\"\n\t\tGET \"/api/health\"\n\t\tstatus = 200\n\n\ttest \"creates user\"\n\t\tPOST \"/api/users\" json(name: \"Alice\", email: \"a@b.com\")\n\t\tstatus = 201\n\t\tjson.name = \"Alice\"\n\n\ttest \"with auth\"\n\t\tGET \"/api/profile\" header(\"Authorization\": \"Bearer token\")\n\t\tstatus = 200\n\t\tjson.email != null\n```\n\n**Assertions:** `=`, `!=`, `<`, `>`, `<=`, `>=`\n\n**Run expression tests:** `cln test <file.cln>`\n\n---\n⚡ *Built-in*".to_string()),
            "plugins" => Some("**plugins:** Block\n\nDeclares plugin dependencies.\n\n```clean\nplugins:\n\tframe.server\n\tframe.data\n\tframe.ui\n\tframe.auth\n```\n\n---\n⚡ *Built-in*".to_string()),
            _ => None,
        }
    }

    fn get_type_info(&self, word: &str) -> Option<String> {
        match word {
            "integer" => Some("**integer** Type\n\nSigned integer number type.\n\n**Examples:**\n```clean\ninteger count = 42\ninteger negative = -10\n```\n\n**Methods:**\n- `toString()` - Convert to string\n- `abs()` - Absolute value\n\n---\n⚡ *Built-in*".to_string()),
            "number" => Some("**number** Type\n\nFloating-point number type.\n\n**Examples:**\n```clean\nnumber pi = 3.14159\nnumber temperature = -5.5\n```\n\n**Methods:**\n- `toString()` - Convert to string\n- `round()` - Round to nearest integer\n- `floor()` - Round down\n- `ceil()` - Round up\n\n---\n⚡ *Built-in*".to_string()),
            "string" => Some("**string** Type\n\nText string type.\n\n**Examples:**\n```clean\nstring name = \"Alice\"\nstring greeting = \"Hello, \" + name\n```\n\n**Methods:**\n- `length()` - Get string length\n- `charAt(index)` - Get character at index\n- `substring(start, end)` - Extract substring\n- `indexOf(text)` - Find text position\n- `replace(old, new)` - Replace all occurrences\n- `toUpperCase()` - Convert to uppercase\n- `toLowerCase()` - Convert to lowercase\n- `trim()` - Remove whitespace\n- `contains(text)` - Check if contains text\n- `split(delimiter)` - Split into list\n\n---\n⚡ *Built-in*".to_string()),
            "boolean" => Some("**boolean** Type\n\nBoolean true/false type.\n\n**Examples:**\n```clean\nboolean isValid = true\nboolean isEmpty = false\n```\n\n**Values:**\n- `true`\n- `false`\n\n---\n⚡ *Built-in*".to_string()),
            "void" => Some("**void** Type\n\nRepresents no return value.\n\n**Usage:**\n```clean\nvoid printMessage(string msg)\n\tprint(msg)\n\t// no return statement needed\n```\n\n---\n⚡ *Built-in*".to_string()),
            "any" => Some("**any** Type\n\nAccepts any type of value.\n\n**Usage:**\n```clean\nany value = 42\nvalue = \"text\"\nvalue = true\n```\n\n**Note:** Use sparingly for type safety.\n\n---\n⚡ *Built-in*".to_string()),
            "list" => Some("**list<T>** Type\n\nDynamic array type.\n\n**Examples:**\n```clean\nlist<integer> numbers = [1, 2, 3]\nlist<string> names = [\"Alice\", \"Bob\"]\ninteger first = numbers[0]\n```\n\n**Methods:**\n- `length()` - Get number of items\n- `add(item)` - Add item to end\n- `remove(index)` - Remove item at index\n- `contains(item)` - Check if list contains item\n- `sort()` - Return sorted list\n\n**List behaviors:**\n```clean\nlist<string> queue = []\nqueue.type = \"line\"    // FIFO queue\nlist<string> stack = []\nstack.type = \"pile\"    // LIFO stack\nlist<string> set = []\nset.type = \"unique\"    // no duplicates\n```\n\n---\n⚡ *Built-in*".to_string()),
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
            "length" => Some("**string.length()** Method\n\nGets the length of the string.\n\n**Returns:** `integer`\n\n**Example:**\n```clean\nstring text = \"Hello\"\ninteger len = text.length()  // 5\n```".to_string()),
            "charAt" => Some("**string.charAt()** Method\n\nGets the character at the specified index.\n\n**Signature:**\n```clean\ncharAt(index: integer) -> string\n```\n\n**Example:**\n```clean\nstring text = \"Hello\"\nstring char = text.charAt(0)  // \"H\"\n```".to_string()),
            "substring" => Some("**string.substring()** Method\n\nExtracts a portion of the string.\n\n**Signature:**\n```clean\nsubstring(start: integer, end: integer) -> string\n```\n\n**Example:**\n```clean\nstring text = \"Hello World\"\nstring sub = text.substring(0, 5)  // \"Hello\"\n```".to_string()),
            _ => None,
        }
    }

    fn get_list_method_info(&self, word: &str) -> Option<String> {
        match word {
            "length" => Some("**list.length()** Method\n\nGets the number of items in the list.\n\n**Returns:** `integer`\n\n**Example:**\n```clean\nlist<integer> nums = [1, 2, 3]\ninteger count = nums.length()  // 3\n```".to_string()),
            "add" => Some("**list.add()** Method\n\nAdds an item to the end of the list.\n\n**Signature:**\n```clean\nadd(item: T) -> void\n```\n\n**Example:**\n```clean\nlist<integer> nums = [1, 2]\nnums.add(3)  // [1, 2, 3]\n```".to_string()),
            "removeLast" => Some("**list.removeLast()** Method\n\nRemoves and returns the last item from the list.\n\n**Signature:**\n```clean\nremoveLast() -> T\n```\n\n**Example:**\n```clean\nlist<integer> nums = [1, 2, 3]\ninteger last = nums.removeLast()  // 3\n```".to_string()),
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
