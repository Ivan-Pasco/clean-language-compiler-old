/*
 * Clean Language Hover Provider
 * Created by Ivan Pasco
 */

use tower_lsp::lsp_types::*;
use ropey::Rope;

pub struct HoverProvider;

impl HoverProvider {
    pub fn new() -> Self {
        Self
    }

    pub async fn provide_hover(&self, text: &Rope, position: Position) -> Option<Hover> {
        let line_idx = position.line as usize;
        
        if line_idx >= text.len_lines() {
            return None;
        }
        
        let line = text.line(line_idx);
        let line_str = line.to_string();
        
        // Simple word extraction at position
        let char_idx = position.character as usize;
        let word = self.extract_word_at_position(&line_str, char_idx)?;
        
        // Provide hover information for known elements
        match word.as_str() {
            // Built-in classes
            "Math" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**Math** - Built-in mathematics class\n\n```clean\nMath.sqrt(number) -> number\nMath.pow(base, exponent) -> number\nMath.sin(angle) -> number\nMath.cos(angle) -> number\nMath.abs(number) -> number\n```\n\nProvides mathematical functions and constants.".to_string(),
                }),
                range: None,
            }),
            "String" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**String** - Built-in string manipulation class\n\n```clean\nString.length(string) -> integer\nString.toUpperCase(string) -> string\nString.toLowerCase(string) -> string\nString.substring(string, start, end) -> string\n```\n\nProvides string processing functions.".to_string(),
                }),
                range: None,
            }),
            "List" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**List** - Built-in list manipulation class\n\n```clean\nList.length(list) -> integer\nList.add(list, item) -> void\nList.filter(list, predicate) -> list\nList.map(list, function) -> list\n```\n\nProvides list operations and functional programming methods.".to_string(),
                }),
                range: None,
            }),
            
            // Keywords
            "start" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**start()** - Main entry point function\n\n```clean\nstart()\n\tprint(\"Hello, World!\")\n```\n\nEvery Clean program must have a `start()` function as its entry point.".to_string(),
                }),
                range: None,
            }),
            "functions" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**functions:** - Function definition block\n\n```clean\nfunctions:\n\tinteger add(integer a, integer b)\n\t\treturn a + b\n```\n\nDefines a block containing function declarations.".to_string(),
                }),
                range: None,
            }),
            "class" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**class** - Class definition keyword\n\n```clean\nclass Person\n\tstring name\n\tinteger age\n\n\tconstructor(string name, integer age)\n\t\tthis.name = name\n\t\tthis.age = age\n```\n\nDefines a new class with fields and methods.".to_string(),
                }),
                range: None,
            }),
            "constant" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**constant:** - Constant definition apply-block\n\n```clean\nconstant:\n\tinteger MAX_SIZE = 100\n\tstring VERSION = \"1.0.0\"\n```\n\nDefines compile-time constants.".to_string(),
                }),
                range: None,
            }),
            "onError" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**onError** - Error handling block\n\n```clean\nfunctionCall() onError errorValue\n\tprint(\"Handling error: \" + errorValue)\n```\n\nHandles errors from function calls or expressions.".to_string(),
                }),
                range: None,
            }),
            "iterate" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**iterate** - Loop over collections\n\n```clean\niterate item in myList\n\tprint(item)\n\n// With index\niterate i in 1 to 10\n\tprint(i)\n```\n\nIterates over lists, ranges, or other iterable collections.".to_string(),
                }),
                range: None,
            }),
            "later" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**later** - Async execution\n\n```clean\nlater result = asyncFunction()\nprint(result)\n```\n\nExecutes operations asynchronously without blocking.".to_string(),
                }),
                range: None,
            }),
            "background" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**background** - Background execution\n\n```clean\nbackground longRunningTask()\n```\n\nExecutes operations in the background without waiting for completion.".to_string(),
                }),
                range: None,
            }),
            
            // Types
            "integer" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**integer** - Signed integer type\n\n```clean\ninteger count = 42\ninteger:32 id = 1000  // 32-bit\ninteger:64 bigNumber = 999999999999\ninteger:8u byte = 255  // 8-bit unsigned\n```\n\nPlatform-optimal signed integer. Supports size specifiers (:8, :16, :32, :64) and unsigned variants (u).".to_string(),
                }),
                range: None,
            }),
            "number" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**number** - Floating-point number type\n\n```clean\nnumber pi = 3.14159\nnumber:32 smallFloat = 1.5  // 32-bit\nnumber:64 doubleFloat = 2.718281828\n```\n\nPlatform-optimal floating-point number (default 64-bit). Supports 32-bit variant with :32 specifier.".to_string(),
                }),
                range: None,
            }),
            "string" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**string** - Text string type\n\n```clean\nstring message = \"Hello, World!\"\nstring interpolated = \"Count: {count}\"\n```\n\nUTF-8 encoded text string with interpolation support using `{variable}` syntax.".to_string(),
                }),
                range: None,
            }),
            "boolean" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**boolean** - True/false type\n\n```clean\nboolean isActive = true\nboolean isEmpty = false\n```\n\nBoolean type with `true` and `false` literal values.".to_string(),
                }),
                range: None,
            }),
            "list" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**list\\<T>** - Generic list type\n\n```clean\nlist<integer> numbers = [1, 2, 3]\nlist<string> names = [\"Alice\", \"Bob\"]\nlist<any> mixed = [1, \"hello\", true]\n```\n\nGeneric list type that can hold elements of any type T.".to_string(),
                }),
                range: None,
            }),
            "matrix" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**matrix\\<T>** - Generic matrix type\n\n```clean\nmatrix<number> grid = [[1.0, 2.0], [3.0, 4.0]]\nmatrix<integer> intMatrix = [[1, 2], [3, 4]]\n```\n\nGeneric matrix (2D array) type for mathematical operations and data grids.".to_string(),
                }),
                range: None,
            }),
            "pairs" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**pairs\\<K, V>** - Generic key-value pairs type\n\n```clean\npairs<string, integer> scores = [(\"Alice\", 100), (\"Bob\", 85)]\npairs<integer, string> mapping = [(1, \"One\"), (2, \"Two\")]\n```\n\nGeneric pairs type for key-value data structures.".to_string(),
                }),
                range: None,
            }),
            "void" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**void** - No return value\n\n```clean\nvoid printMessage(string msg)\n\tprint(msg)\n\t// No return statement needed\n```\n\nIndicates that a function does not return a value.".to_string(),
                }),
                range: None,
            }),
            "any" => Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: "**any** - Universal type\n\n```clean\nany value = 42\nvalue = \"Hello\"\nvalue = [1, 2, 3]\n```\n\nUniversal type that can hold any value. Use sparingly for type safety.".to_string(),
                }),
                range: None,
            }),
            
            _ => None,
        }
    }

    fn extract_word_at_position(&self, line: &str, char_idx: usize) -> Option<String> {
        if char_idx > line.len() {
            return None;
        }
        
        let chars: Vec<char> = line.chars().collect();
        
        // Find word boundaries
        let mut start = char_idx;
        let mut end = char_idx;
        
        // Move start backwards
        while start > 0 && chars[start - 1].is_alphanumeric() {
            start -= 1;
        }
        
        // Move end forwards
        while end < chars.len() && chars[end].is_alphanumeric() {
            end += 1;
        }
        
        if start < end {
            Some(chars[start..end].iter().collect())
        } else {
            None
        }
    }
}