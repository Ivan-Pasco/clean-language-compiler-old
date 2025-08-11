# Clean Language Advanced Debugging & Development Tools

Welcome to the advanced debugging capabilities of Clean Language! This section covers the powerful development tools that make writing, debugging, and maintaining Clean Language code a delightful experience.

## 🎯 Overview

Clean Language provides a comprehensive suite of debugging and development tools designed to help you write better code faster. Whether you're a beginner learning the language or an experienced developer building complex applications, these tools will become your best friends.

## 🚀 Getting Started with Debugging Tools

### Quick Start
The easiest way to access debugging features is through our friendly command-line interface:

```bash
# Debug your code with detailed analysis
cargo run --bin clean-language-compiler -- debug --input my_code.clean

# Check your code style and get helpful suggestions
cargo run --bin clean-language-compiler -- lint --input my_project/

# Parse and analyze your code structure
cargo run --bin clean-language-compiler -- parse --input my_code.clean --show-tree
```

## 🔍 Debug Command - Your Code Detective

The `debug` command is like having a friendly code reviewer who never gets tired and always has helpful suggestions.

### Basic Usage
```bash
cargo run --bin clean-language-compiler -- debug --input your_file.clean
```

### Available Options

#### `--show-ast` - See Your Code's Structure
Ever wondered how Clean Language understands your code? This option shows you the Abstract Syntax Tree (AST) - think of it as an X-ray of your program's structure.

```bash
cargo run --bin clean-language-compiler -- debug --input my_code.clean --show-ast
```

**What you'll see:**
- Function hierarchies with proper indentation
- Parameter lists and types
- Statement summaries
- Class structures

**Perfect for:**
- Understanding complex code structures
- Learning how Clean Language parses your code
- Debugging parser-related issues

#### `--check-style` - Your Personal Style Coach
Clean Language believes code should be beautiful and consistent. This option checks your code against Clean Language style conventions and gives you friendly suggestions.

```bash
cargo run --bin clean-language-compiler -- debug --input my_code.clean --check-style
```

**What it checks:**
- ✅ camelCase naming conventions for functions and variables
- ✅ Proper indentation (Clean Language loves tabs!)
- ✅ Line length recommendations
- ✅ Trailing whitespace cleanup
- ✅ Missing function descriptions

#### `--analyze-errors` - Error Whisperer
When things go wrong, this option doesn't just tell you what's broken - it helps you understand why and how to fix it.

```bash
cargo run --bin clean-language-compiler -- debug --input problematic_code.clean --analyze-errors
```

**What you get:**
- 💡 Contextual help based on error type
- 🔧 Specific suggestions for common mistakes
- 📚 References to relevant documentation
- 🎯 Step-by-step guidance for fixes

### Combine Options for Maximum Power
```bash
# Get the full debugging experience
cargo run --bin clean-language-compiler -- debug --input my_code.clean --show-ast --check-style --analyze-errors
```

## 🧹 Lint Command - Your Code Quality Guardian

The `lint` command is like having a meticulous friend who helps keep your code clean and consistent across your entire project.

### Basic Usage
```bash
# Lint a single file
cargo run --bin clean-language-compiler -- lint --input my_file.clean

# Lint an entire directory (finds all .clean files)
cargo run --bin clean-language-compiler -- lint --input my_project/
```

### Available Options

#### `--errors-only` - Focus Mode
When you're in the zone and just want to see the critical issues:

```bash
cargo run --bin clean-language-compiler -- lint --input my_project/ --errors-only
```

#### `--fix` - Auto-Magic Cleanup (Coming Soon!)
Future feature that will automatically fix common style issues:

```bash
cargo run --bin clean-language-compiler -- lint --input my_project/ --fix
```

### What Lint Checks For

**Compilation Issues:**
- Syntax errors with helpful context
- Type mismatches with suggestions
- Missing imports or dependencies

**Style Issues:**
- Naming convention violations
- Indentation inconsistencies
- Code formatting problems
- Line length recommendations

**Best Practices:**
- Missing function descriptions
- Overly complex functions
- Deeply nested code structures

## 📝 Parse Command - Your Code Analyzer

The `parse` command is perfect for understanding how Clean Language interprets your code and for debugging parsing issues.

### Basic Usage
```bash
cargo run --bin clean-language-compiler -- parse --input my_code.clean
```

### Available Options

#### `--show-tree` - Code Structure Visualization
See exactly how Clean Language breaks down your code:

```bash
cargo run --bin clean-language-compiler -- parse --input my_code.clean --show-tree
```

**Great for:**
- Understanding parsing behavior
- Learning Clean Language syntax
- Debugging complex expressions
- Educational purposes

#### `--recover-errors` - Resilient Parsing
When your code has errors, this mode tries to parse as much as possible and gives you comprehensive feedback:

```bash
cargo run --bin clean-language-compiler -- parse --input broken_code.clean --recover-errors
```

**Benefits:**
- See multiple errors at once
- Get context for each error
- Understand error relationships
- Faster debugging cycles

## 🛠 Advanced API Usage

For developers who want to integrate Clean Language debugging into their own tools, we provide a powerful Rust API.

### Setting Up the Library

Add to your `Cargo.toml`:
```toml
[dependencies]
clean-language-compiler = { path = "path/to/clean-language" }
```

### Basic API Usage

```rust
use clean_language_compiler::debug::DebugUtils;
use clean_language_compiler::parser::CleanParser;

fn analyze_clean_code(source: &str, file_path: &str) {
    // Parse the code
    match CleanParser::parse_program_with_file(source, file_path) {
        Ok(program) => {
            // Analyze code complexity
            let complexity_issues = DebugUtils::analyze_complexity(&program);
            for issue in complexity_issues {
                println!("💡 Complexity: {}", issue);
            }
            
            // Check code style
            let style_issues = DebugUtils::validate_style(source);
            for issue in style_issues {
                println!("🎨 Style: {}", issue);
            }
            
            // Print AST structure
            DebugUtils::print_ast(&program);
        },
        Err(error) => {
            // Analyze the error
            DebugUtils::analyze_error(&error);
        }
    }
}
```

### Available API Methods

#### AST Analysis
```rust
// Pretty-print the entire program structure
DebugUtils::print_ast(&program);

// Analyze code complexity and get suggestions
let suggestions = DebugUtils::analyze_complexity(&program);
```

#### Style Validation
```rust
// Quick style check
let issues = DebugUtils::validate_style(source_code);

// Comprehensive style report
let report = DebugUtils::generate_style_report(&program);
report.print();
```

#### Error Analysis
```rust
// Analyze a single error with context
DebugUtils::analyze_error(&compiler_error);

// Analyze multiple errors
let analysis = DebugUtils::analyze_errors(&error_list);
```

#### Comprehensive Reporting
```rust
// Create a detailed debug report
let report = DebugUtils::create_debug_report(
    source_code,
    file_path,
    &parse_result,
    &warnings
);
println!("{}", report);
```

## 🎨 Integration Examples

### IDE Plugin Development
```rust
// Language server integration
use clean_language_compiler::debug::DebugUtils;

fn provide_diagnostics(source: &str) -> Vec<Diagnostic> {
    let style_issues = DebugUtils::validate_style(source);
    style_issues.into_iter()
        .map(|issue| Diagnostic::new_warning(issue))
        .collect()
}

fn provide_code_actions(program: &Program) -> Vec<CodeAction> {
    let complexity_issues = DebugUtils::analyze_complexity(program);
    complexity_issues.into_iter()
        .map(|issue| CodeAction::new_refactor(issue))
        .collect()
}
```

### Build Tool Integration
```rust
// Custom build script
use clean_language_compiler::debug::DebugUtils;
use std::fs;

fn lint_project(project_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let clean_files = find_clean_files(project_dir)?;
    
    for file_path in clean_files {
        let source = fs::read_to_string(&file_path)?;
        let issues = DebugUtils::validate_style(&source);
        
        if !issues.is_empty() {
            println!("📁 {}", file_path);
            for issue in issues {
                println!("  🎨 {}", issue);
            }
        }
    }
    
    Ok(())
}
```

### Testing Framework Integration
```rust
// Custom test runner with debugging
use clean_language_compiler::debug::DebugUtils;

fn run_test_with_debugging(test_code: &str) -> TestResult {
    match CleanParser::parse_program_with_file(test_code, "test.clean") {
        Ok(program) => {
            // Check for complexity issues in test code
            let complexity = DebugUtils::analyze_complexity(&program);
            if !complexity.is_empty() {
                println!("⚠️ Test complexity warnings:");
                for warning in complexity {
                    println!("  {}", warning);
                }
            }
            
            // Run the actual test...
            run_test(program)
        },
        Err(error) => {
            DebugUtils::analyze_error(&error);
            TestResult::ParseError(error)
        }
    }
}
```

## 🎯 Best Practices

### For Daily Development
1. **Use `debug` command regularly** - Make it part of your development workflow
2. **Run `lint` before commits** - Keep your codebase clean and consistent
3. **Use `--show-ast` for learning** - Great way to understand Clean Language better
4. **Combine options** - Get comprehensive analysis with multiple flags

### For Team Development
1. **Integrate linting in CI/CD** - Ensure consistent code quality
2. **Share debug reports** - Help teammates understand complex issues
3. **Use style checking** - Maintain consistent coding standards
4. **Document debugging workflows** - Help new team members get up to speed

### For Tool Development
1. **Use the API directly** - Build custom tools for your specific needs
2. **Combine multiple methods** - Create comprehensive analysis tools
3. **Handle errors gracefully** - Provide helpful feedback to users
4. **Cache results** - Improve performance for large codebases

## 🚀 Future Enhancements

We're constantly improving the debugging experience! Here's what's coming:

### Planned Features
- **Auto-fix capabilities** - Automatic correction of common style issues
- **Performance profiling** - Identify bottlenecks in your Clean Language code
- **Interactive debugging** - Step-through debugging for complex logic
- **Visual AST explorer** - Graphical representation of code structure
- **Code metrics dashboard** - Comprehensive code quality metrics
- **Refactoring suggestions** - AI-powered code improvement recommendations

### Community Contributions
We love community contributions! Here are ways you can help:

- **Report bugs** - Help us improve the debugging tools
- **Suggest features** - Tell us what debugging features you need
- **Contribute code** - Add new analysis methods or improve existing ones
- **Write documentation** - Help other developers learn these tools
- **Create integrations** - Build plugins for your favorite editors

## 🎉 Conclusion

Clean Language's debugging tools are designed to make your development experience as smooth and enjoyable as possible. Whether you're debugging a tricky issue, maintaining code quality, or learning the language, these tools are here to help.

Remember: good debugging tools don't just find problems - they help you understand your code better and become a more effective developer. Happy coding! 🚀

---

*For more information, examples, and updates, visit our documentation or join our community discussions.*
 