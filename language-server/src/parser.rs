/*
 * Clean Language Parser for LSP
 * Created by Ivan Pasco
 * 
 * This module provides parsing capabilities for the Clean Language LSP server,
 * including error recovery and AST generation.
 */

use tower_lsp::lsp_types::{Position, Range};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct ParseError {
    pub range: Range,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum CleanASTNode {
    Program {
        functions: Vec<CleanASTNode>,
        classes: Vec<CleanASTNode>,
        start_function: Option<Box<CleanASTNode>>,
    },
    Function {
        name: String,
        parameters: Vec<Parameter>,
        return_type: Option<String>,
        body: Vec<CleanASTNode>,
        range: Range,
    },
    Class {
        name: String,
        extends: Option<String>,
        fields: Vec<CleanASTNode>,
        constructor: Option<Box<CleanASTNode>>,
        methods: Vec<CleanASTNode>,
        range: Range,
    },
    ApplyBlock {
        target: String,
        items: Vec<CleanASTNode>,
        range: Range,
    },
    ConstantApplyBlock {
        assignments: Vec<CleanASTNode>,
        range: Range,
    },
    TypeApplyBlock {
        type_name: String,
        assignments: Vec<CleanASTNode>,
        range: Range,
    },
    MethodApplyBlock {
        method_chain: String,
        expressions: Vec<CleanASTNode>,
        range: Range,
    },
    FunctionApplyBlock {
        function_name: String,
        expressions: Vec<CleanASTNode>,
        range: Range,
    },
    VariableDeclaration {
        var_type: String,
        name: String,
        value: Option<Box<CleanASTNode>>,
        range: Range,
    },
    MethodCall {
        object: Option<String>,
        method: String,
        args: Vec<CleanASTNode>,
        range: Range,
    },
    Literal {
        value: String,
        literal_type: String,
        range: Range,
    },
    Identifier {
        name: String,
        range: Range,
    },
    StringInterpolation {
        parts: Vec<CleanASTNode>,
        range: Range,
    },
    BinaryOperation {
        left: Box<CleanASTNode>,
        operator: String,
        right: Box<CleanASTNode>,
        range: Range,
    },
    UnaryOperation {
        operator: String,
        operand: Box<CleanASTNode>,
        range: Range,
    },
    IfStatement {
        condition: Box<CleanASTNode>,
        then_branch: Vec<CleanASTNode>,
        else_branch: Option<Vec<CleanASTNode>>,
        range: Range,
    },
    IterateStatement {
        variable: String,
        iterable: Box<CleanASTNode>,
        body: Vec<CleanASTNode>,
        range: Range,
    },
    ImportStatement {
        module: String,
        items: Option<Vec<String>>,
        alias: Option<String>,
        range: Range,
    },
    TestBlock {
        name: String,
        body: Vec<CleanASTNode>,
        range: Range,
    },
    ErrorHandling {
        try_block: Vec<CleanASTNode>,
        error_handlers: Vec<CleanASTNode>,
        range: Range,
    },
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub param_type: String,
    pub name: String,
    pub default_value: Option<String>,
}

pub struct CleanParser {
    keywords: HashMap<String, KeywordType>,
    builtin_classes: HashMap<String, BuiltinClass>,
}

#[derive(Debug, Clone)]
enum KeywordType {
    Control,
    Type,
    Function,
    Class,
}

#[derive(Debug, Clone)]
struct BuiltinClass {
    name: String,
    methods: Vec<BuiltinMethod>,
}

#[derive(Debug, Clone)]
struct BuiltinMethod {
    name: String,
    parameters: Vec<String>,
    return_type: String,
    description: String,
}

impl CleanParser {
    pub fn new() -> Self {
        let mut keywords = HashMap::new();
        
        // Control keywords from compiler grammar
        keywords.insert("if".to_string(), KeywordType::Control);
        keywords.insert("else".to_string(), KeywordType::Control);
        keywords.insert("then".to_string(), KeywordType::Control);
        keywords.insert("iterate".to_string(), KeywordType::Control);
        keywords.insert("in".to_string(), KeywordType::Control);
        keywords.insert("to".to_string(), KeywordType::Control);
        keywords.insert("step".to_string(), KeywordType::Control);
        keywords.insert("return".to_string(), KeywordType::Control);
        keywords.insert("error".to_string(), KeywordType::Control);
        keywords.insert("onError".to_string(), KeywordType::Control);
        keywords.insert("and".to_string(), KeywordType::Control);
        keywords.insert("or".to_string(), KeywordType::Control);
        keywords.insert("not".to_string(), KeywordType::Control);
        keywords.insert("is".to_string(), KeywordType::Control);
        keywords.insert("true".to_string(), KeywordType::Control);
        keywords.insert("false".to_string(), KeywordType::Control);
        
        // Type keywords from compiler grammar
        keywords.insert("integer".to_string(), KeywordType::Type);
        keywords.insert("number".to_string(), KeywordType::Type);
        keywords.insert("string".to_string(), KeywordType::Type);
        keywords.insert("boolean".to_string(), KeywordType::Type);
        keywords.insert("list".to_string(), KeywordType::Type);
        keywords.insert("matrix".to_string(), KeywordType::Type);
        keywords.insert("pairs".to_string(), KeywordType::Type);
        keywords.insert("void".to_string(), KeywordType::Type);
        keywords.insert("any".to_string(), KeywordType::Type);
        
        // Function keywords from compiler grammar  
        keywords.insert("function".to_string(), KeywordType::Function);
        keywords.insert("functions".to_string(), KeywordType::Function);
        keywords.insert("start".to_string(), KeywordType::Function);
        keywords.insert("input".to_string(), KeywordType::Function);
        keywords.insert("description".to_string(), KeywordType::Function);
        keywords.insert("tests".to_string(), KeywordType::Function);
        
        // Class keywords
        keywords.insert("class".to_string(), KeywordType::Class);
        keywords.insert("constructor".to_string(), KeywordType::Class);
        keywords.insert("constant".to_string(), KeywordType::Class);
        keywords.insert("private".to_string(), KeywordType::Class);
        keywords.insert("base".to_string(), KeywordType::Class);
        
        // Module keywords
        keywords.insert("import".to_string(), KeywordType::Function);
        keywords.insert("from".to_string(), KeywordType::Function);
        keywords.insert("as".to_string(), KeywordType::Function);
        
        // Async keywords
        keywords.insert("later".to_string(), KeywordType::Control);
        keywords.insert("background".to_string(), KeywordType::Control);
        
        let mut builtin_classes = HashMap::new();
        
        // Math class
        builtin_classes.insert("Math".to_string(), BuiltinClass {
            name: "Math".to_string(),
            methods: vec![
                BuiltinMethod {
                    name: "sqrt".to_string(),
                    parameters: vec!["number".to_string()],
                    return_type: "number".to_string(),
                    description: "Returns the square root of a number".to_string(),
                },
                BuiltinMethod {
                    name: "pow".to_string(),
                    parameters: vec!["number".to_string(), "number".to_string()],
                    return_type: "number".to_string(),
                    description: "Returns base raised to the power of exponent".to_string(),
                },
                BuiltinMethod {
                    name: "abs".to_string(),
                    parameters: vec!["number".to_string()],
                    return_type: "number".to_string(),
                    description: "Returns the absolute value of a number".to_string(),
                },
            ],
        });
        
        // String class
        builtin_classes.insert("String".to_string(), BuiltinClass {
            name: "String".to_string(),
            methods: vec![
                BuiltinMethod {
                    name: "length".to_string(),
                    parameters: vec!["string".to_string()],
                    return_type: "integer".to_string(),
                    description: "Returns the length of a string".to_string(),
                },
                BuiltinMethod {
                    name: "toUpperCase".to_string(),
                    parameters: vec!["string".to_string()],
                    return_type: "string".to_string(),
                    description: "Converts string to uppercase".to_string(),
                },
                BuiltinMethod {
                    name: "toLowerCase".to_string(),
                    parameters: vec!["string".to_string()],
                    return_type: "string".to_string(),
                    description: "Converts string to lowercase".to_string(),
                },
            ],
        });
        
        // List class
        builtin_classes.insert("List".to_string(), BuiltinClass {
            name: "List".to_string(),
            methods: vec![
                BuiltinMethod {
                    name: "length".to_string(),
                    parameters: vec!["list<any>".to_string()],
                    return_type: "integer".to_string(),
                    description: "Returns the length of a list".to_string(),
                },
                BuiltinMethod {
                    name: "add".to_string(),
                    parameters: vec!["list<any>".to_string(), "any".to_string()],
                    return_type: "void".to_string(),
                    description: "Adds an item to the list".to_string(),
                },
                BuiltinMethod {
                    name: "filter".to_string(),
                    parameters: vec!["list<any>".to_string(), "function".to_string()],
                    return_type: "list<any>".to_string(),
                    description: "Filters list items based on a predicate".to_string(),
                },
            ],
        });
        
        // Http class
        builtin_classes.insert("Http".to_string(), BuiltinClass {
            name: "Http".to_string(),
            methods: vec![
                BuiltinMethod {
                    name: "get".to_string(),
                    parameters: vec!["string".to_string()],
                    return_type: "string".to_string(),
                    description: "Performs an HTTP GET request".to_string(),
                },
                BuiltinMethod {
                    name: "post".to_string(),
                    parameters: vec!["string".to_string(), "string".to_string()],
                    return_type: "string".to_string(),
                    description: "Performs an HTTP POST request".to_string(),
                },
            ],
        });
        
        // File class
        builtin_classes.insert("File".to_string(), BuiltinClass {
            name: "File".to_string(),
            methods: vec![
                BuiltinMethod {
                    name: "read".to_string(),
                    parameters: vec!["string".to_string()],
                    return_type: "string".to_string(),
                    description: "Reads content from a file".to_string(),
                },
                BuiltinMethod {
                    name: "write".to_string(),
                    parameters: vec!["string".to_string(), "string".to_string()],
                    return_type: "void".to_string(),
                    description: "Writes content to a file".to_string(),
                },
            ],
        });
        
        Self {
            keywords,
            builtin_classes,
        }
    }
    
    pub async fn parse(&self, text: &str) -> Result<CleanASTNode, Vec<ParseError>> {
        let mut errors = Vec::new();
        let lines: Vec<&str> = text.lines().collect();
        
        // Basic parsing implementation
        // This is a simplified parser - in practice, you'd want to integrate
        // with the actual Clean Language parser from the compiler project
        
        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut start_function = None;
        
        let mut current_line = 0;
        
        while current_line < lines.len() {
            let line = lines[current_line].trim();
            
            if line.is_empty() || line.starts_with("//") {
                current_line += 1;
                continue;
            }
            
            if line.starts_with("function") {
                match self.parse_function(&lines, &mut current_line) {
                    Ok(func) => {
                        if let CleanASTNode::Function { name, .. } = &func {
                            if name == "start" {
                                start_function = Some(Box::new(func));
                            } else {
                                functions.push(func);
                            }
                        }
                    }
                    Err(err) => errors.push(err),
                }
            } else if line.starts_with("class") {
                match self.parse_class(&lines, &mut current_line) {
                    Ok(class) => classes.push(class),
                    Err(err) => errors.push(err),
                }
            } else {
                current_line += 1;
            }
        }
        
        if errors.is_empty() {
            Ok(CleanASTNode::Program {
                functions,
                classes,
                start_function,
            })
        } else {
            Err(errors)
        }
    }
    
    fn parse_function(&self, lines: &[&str], current_line: &mut usize) -> Result<CleanASTNode, ParseError> {
        let line = lines[*current_line];
        let start_line = *current_line;
        
        // Parse function declaration: function name(params)
        // This is a simplified implementation
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(ParseError {
                range: Range {
                    start: Position { line: start_line as u32, character: 0 },
                    end: Position { line: start_line as u32, character: line.len() as u32 },
                },
                message: "Invalid function declaration".to_string(),
            });
        }
        
        let name = parts[1].split('(').next().unwrap_or("").to_string();
        *current_line += 1;
        
        // Parse function body (simplified)
        let mut body = Vec::new();
        while *current_line < lines.len() {
            let line = lines[*current_line].trim();
            if line.is_empty() || (!line.starts_with('\t') && !line.starts_with(' ')) {
                break;
            }
            *current_line += 1;
        }
        
        Ok(CleanASTNode::Function {
            name,
            parameters: Vec::new(), // Simplified
            return_type: None,
            body,
            range: Range {
                start: Position { line: start_line as u32, character: 0 },
                end: Position { line: *current_line as u32, character: 0 },
            },
        })
    }
    
    fn parse_class(&self, lines: &[&str], current_line: &mut usize) -> Result<CleanASTNode, ParseError> {
        let line = lines[*current_line];
        let start_line = *current_line;
        
        // Parse class declaration: class Name [is ParentClass]
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(ParseError {
                range: Range {
                    start: Position { line: start_line as u32, character: 0 },
                    end: Position { line: start_line as u32, character: line.len() as u32 },
                },
                message: "Invalid class declaration".to_string(),
            });
        }
        
        let name = parts[1].to_string();
        let extends = if parts.len() > 3 && parts[2] == "is" {
            Some(parts[3].to_string())
        } else {
            None
        };
        
        *current_line += 1;
        
        // Parse class body (simplified)
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let constructor = None; // Simplified
        
        while *current_line < lines.len() {
            let line = lines[*current_line].trim();
            if line.is_empty() || (!line.starts_with('\t') && !line.starts_with(' ')) {
                break;
            }
            *current_line += 1;
        }
        
        Ok(CleanASTNode::Class {
            name,
            extends,
            fields,
            constructor,
            methods,
            range: Range {
                start: Position { line: start_line as u32, character: 0 },
                end: Position { line: *current_line as u32, character: 0 },
            },
        })
    }
    
    pub fn get_builtin_classes(&self) -> &HashMap<String, BuiltinClass> {
        &self.builtin_classes
    }
    
    pub fn is_keyword(&self, word: &str) -> bool {
        self.keywords.contains_key(word)
    }
}