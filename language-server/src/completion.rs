/*
 * Clean Language Completion Provider
 * Created by Ivan Pasco
 * 
 * This module provides intelligent autocompletion for Clean Language including:
 * - Keywords and syntax patterns
 * - Built-in class methods
 * - Type annotations
 * - Apply-block completions
 */

use tower_lsp::lsp_types::*;
use ropey::Rope;

pub struct CompletionProvider;

impl CompletionProvider {
    pub fn new() -> Self {
        Self
    }

    pub async fn provide_completions(&self, text: &Rope, position: Position) -> Vec<CompletionItem> {
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
            // Method completion after dot
            completions.extend(self.get_method_completions(prefix));
        } else if self.is_apply_block_context(prefix) {
            // Apply-block completions
            completions.extend(self.get_apply_block_completions());
        } else if self.is_type_context(prefix) {
            // Type completions
            completions.extend(self.get_type_completions());
        } else {
            // General keyword and identifier completions
            completions.extend(self.get_keyword_completions());
            completions.extend(self.get_builtin_class_completions());
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
        let words: Vec<&str> = prefix.split_whitespace().collect();
        // Check if we're in a variable declaration context
        words.len() == 1 && (words[0] == "integer" || words[0] == "number" || words[0] == "string" || words[0] == "boolean")
    }

    fn get_method_completions(&self, prefix: &str) -> Vec<CompletionItem> {
        let mut completions = Vec::new();
        
        // Extract the object before the dot
        let parts: Vec<&str> = prefix.split('.').collect();
        if parts.len() < 2 {
            return completions;
        }
        
        let object_part = parts[parts.len() - 2].split_whitespace().last().unwrap_or("");
        
        match object_part {
            "Math" => {
                completions.extend(vec![
                    CompletionItem {
                        label: "sqrt".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("sqrt(number) -> number".to_string()),
                        documentation: Some(Documentation::String("Returns the square root of a number".to_string())),
                        insert_text: Some("sqrt($1)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "pow".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("pow(base, exponent) -> number".to_string()),
                        documentation: Some(Documentation::String("Returns base raised to the power of exponent".to_string())),
                        insert_text: Some("pow($1, $2)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "abs".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("abs(number) -> number".to_string()),
                        documentation: Some(Documentation::String("Returns the absolute value of a number".to_string())),
                        insert_text: Some("abs($1)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "sin".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("sin(angle) -> number".to_string()),
                        documentation: Some(Documentation::String("Returns the sine of an angle (in radians)".to_string())),
                        insert_text: Some("sin($1)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "cos".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("cos(angle) -> number".to_string()),
                        documentation: Some(Documentation::String("Returns the cosine of an angle (in radians)".to_string())),
                        insert_text: Some("cos($1)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                ]);
            },
            "String" => {
                completions.extend(vec![
                    CompletionItem {
                        label: "length".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("length(string) -> integer".to_string()),
                        documentation: Some(Documentation::String("Returns the length of a string".to_string())),
                        insert_text: Some("length($1)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "toUpperCase".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("toUpperCase(string) -> string".to_string()),
                        documentation: Some(Documentation::String("Converts string to uppercase".to_string())),
                        insert_text: Some("toUpperCase($1)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "toLowerCase".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("toLowerCase(string) -> string".to_string()),
                        documentation: Some(Documentation::String("Converts string to lowercase".to_string())),
                        insert_text: Some("toLowerCase($1)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "substring".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("substring(string, start, end) -> string".to_string()),
                        documentation: Some(Documentation::String("Returns a substring from start to end".to_string())),
                        insert_text: Some("substring($1, $2, $3)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                ]);
            },
            "List" => {
                completions.extend(vec![
                    CompletionItem {
                        label: "length".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("length(list) -> integer".to_string()),
                        documentation: Some(Documentation::String("Returns the length of a list".to_string())),
                        insert_text: Some("length($1)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "add".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("add(list, item) -> void".to_string()),
                        documentation: Some(Documentation::String("Adds an item to the list".to_string())),
                        insert_text: Some("add($1, $2)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "filter".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("filter(list, predicate) -> list".to_string()),
                        documentation: Some(Documentation::String("Filters list items based on a predicate function".to_string())),
                        insert_text: Some("filter($1, $2)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                ]);
            },
            "Http" => {
                completions.extend(vec![
                    CompletionItem {
                        label: "get".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("get(url) -> string".to_string()),
                        documentation: Some(Documentation::String("Performs an HTTP GET request".to_string())),
                        insert_text: Some("get(\"$1\")".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "post".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("post(url, data) -> string".to_string()),
                        documentation: Some(Documentation::String("Performs an HTTP POST request".to_string())),
                        insert_text: Some("post(\"$1\", $2)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                ]);
            },
            "File" => {
                completions.extend(vec![
                    CompletionItem {
                        label: "read".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("read(filename) -> string".to_string()),
                        documentation: Some(Documentation::String("Reads content from a file".to_string())),
                        insert_text: Some("read(\"$1\")".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                    CompletionItem {
                        label: "write".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("write(filename, content) -> void".to_string()),
                        documentation: Some(Documentation::String("Writes content to a file".to_string())),
                        insert_text: Some("write(\"$1\", $2)".to_string()),
                        insert_text_format: Some(InsertTextFormat::SNIPPET),
                        ..Default::default()
                    },
                ]);
            },
            _ => {
                // For user-defined objects, provide common method patterns
                completions.extend(vec![
                    CompletionItem {
                        label: "toString".to_string(),
                        kind: Some(CompletionItemKind::METHOD),
                        detail: Some("toString() -> string".to_string()),
                        documentation: Some(Documentation::String("Converts object to string representation".to_string())),
                        insert_text: Some("toString()".to_string()),
                        ..Default::default()
                    },
                ]);
            }
        }
        
        completions
    }

    fn get_apply_block_completions(&self) -> Vec<CompletionItem> {
        vec![
            // Function apply-blocks
            CompletionItem {
                label: "print".to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some("print apply-block".to_string()),
                documentation: Some(Documentation::String("Print items to console".to_string())),
                insert_text: Some("\n\t$1".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "println".to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some("println apply-block".to_string()),
                documentation: Some(Documentation::String("Print items to console with newline".to_string())),
                insert_text: Some("\n\t$1".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            // Type apply-blocks
            CompletionItem {
                label: "integer".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("integer type apply-block".to_string()),
                documentation: Some(Documentation::String("Declare multiple integer variables".to_string())),
                insert_text: Some("\n\t$1 = $2".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "string".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("string type apply-block".to_string()),
                documentation: Some(Documentation::String("Declare multiple string variables".to_string())),
                insert_text: Some("\n\t$1 = \"$2\"".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "number".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("number type apply-block".to_string()),
                documentation: Some(Documentation::String("Declare multiple number variables".to_string())),
                insert_text: Some("\n\t$1 = $2".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "list".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("list type apply-block".to_string()),
                documentation: Some(Documentation::String("Declare multiple list variables".to_string())),
                insert_text: Some("\n\t$1 = [$2]".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            // Constant apply-blocks
            CompletionItem {
                label: "constant".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("constant apply-block".to_string()),
                documentation: Some(Documentation::String("Define constant values".to_string())),
                insert_text: Some("\n\t$1 $2 = $3".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
        ]
    }

    fn get_type_completions(&self) -> Vec<CompletionItem> {
        vec![
            CompletionItem {
                label: "integer".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("Platform-optimal signed integer".to_string()),
                insert_text: Some("integer".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "integer:8".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("8-bit signed integer".to_string()),
                insert_text: Some("integer:8".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "integer:8u".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("8-bit unsigned integer".to_string()),
                insert_text: Some("integer:8u".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "integer:16".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("16-bit signed integer".to_string()),
                insert_text: Some("integer:16".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "integer:16u".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("16-bit unsigned integer".to_string()),
                insert_text: Some("integer:16u".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "integer:32".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("32-bit signed integer".to_string()),
                insert_text: Some("integer:32".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "integer:64".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("64-bit signed integer".to_string()),
                insert_text: Some("integer:64".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "number".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("64-bit floating point number".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "number:32".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("32-bit floating point number".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "string".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("String type".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "boolean".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("Boolean type (true/false)".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "list<T>".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("Generic list type".to_string()),
                insert_text: Some("list<$1>".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "matrix<T>".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("Generic matrix type".to_string()),
                insert_text: Some("matrix<$1>".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "pairs<T, U>".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("Generic pairs type for key-value pairs".to_string()),
                insert_text: Some("pairs<$1, $2>".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "any".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("Universal generic type".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "void".to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                detail: Some("No return value".to_string()),
                ..Default::default()
            },
        ]
    }

    fn get_keyword_completions(&self) -> Vec<CompletionItem> {
        vec![
            // Control flow
            CompletionItem {
                label: "if".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("if $1\n\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "else".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("else\n\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "iterate".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("iterate $1 in $2\n\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "while".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("while $1\n\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "for".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("for $1 = $2 to $3\n\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            // Function definitions
            CompletionItem {
                label: "function".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("function $1($2)\n\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "functions".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("functions:\n\t$1 $2($3)\n\t\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "start".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("start()\n\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            // Class definitions
            CompletionItem {
                label: "class".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("class $1\n\t$2 $3\n\n\tconstructor($4)\n\n\tfunctions:\n\t\t$5 $6($7)\n\t\t\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "constructor".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("constructor($1)\n\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            // Testing
            CompletionItem {
                label: "tests".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("tests:\n\t\"$1\": $2 = $3\n\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            // Error handling
            CompletionItem {
                label: "onError".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("onError $1\n\t$0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                detail: Some("Error handling block".to_string()),
                ..Default::default()
            },
            // Async keywords
            CompletionItem {
                label: "later".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("later $0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                detail: Some("Async execution".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "background".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("background $0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                detail: Some("Background execution".to_string()),
                ..Default::default()
            },
            // Module keywords
            CompletionItem {
                label: "import".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("import $1 from \"$2\"".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                detail: Some("Import statement".to_string()),
                ..Default::default()
            },
            // Class modifiers
            CompletionItem {
                label: "constant".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("constant:\n\t$1 $2 = $3".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                detail: Some("Constant apply-block".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "private".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("private $0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                detail: Some("Private member".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "base".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("base($0)".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                detail: Some("Base class constructor call".to_string()),
                ..Default::default()
            },
            // Function metadata
            CompletionItem {
                label: "input".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("input:\n\t$1: $2".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                detail: Some("Function input specification".to_string()),
                ..Default::default()
            },
            CompletionItem {
                label: "description".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("description \"$1\"".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                detail: Some("Function description".to_string()),
                ..Default::default()
            },
            // Other keywords
            CompletionItem {
                label: "return".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("return $0".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
            CompletionItem {
                label: "error".to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                insert_text: Some("error \"$1\"".to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            },
        ]
    }

    fn get_builtin_class_completions(&self) -> Vec<CompletionItem> {
        vec![
            CompletionItem {
                label: "Math".to_string(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some("Built-in Math class".to_string()),
                documentation: Some(Documentation::String("Provides mathematical functions and constants".to_string())),
                ..Default::default()
            },
            CompletionItem {
                label: "String".to_string(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some("Built-in String class".to_string()),
                documentation: Some(Documentation::String("Provides string manipulation functions".to_string())),
                ..Default::default()
            },
            CompletionItem {
                label: "List".to_string(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some("Built-in List class".to_string()),
                documentation: Some(Documentation::String("Provides list manipulation functions".to_string())),
                ..Default::default()
            },
            CompletionItem {
                label: "Http".to_string(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some("Built-in Http class".to_string()),
                documentation: Some(Documentation::String("Provides HTTP request functions".to_string())),
                ..Default::default()
            },
            CompletionItem {
                label: "File".to_string(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some("Built-in File class".to_string()),
                documentation: Some(Documentation::String("Provides file I/O functions".to_string())),
                ..Default::default()
            },
        ]
    }
}